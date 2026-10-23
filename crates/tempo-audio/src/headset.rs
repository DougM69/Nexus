//! This computer's headset on its own sound devices, for a shack it drives over its own network
//! (client mode). Its rings, its cushion, the page's level and its sounds are tempo-codec's
//! (`tempo_codec::headset`), named here as they always were; this is the part on cpal's devices.
//!
//! ## Open only while wanted (`Headset`, with the `device` feature)
//!
//! One thread of the headset's own opens and holds the devices (a cpal stream lives and dies on
//! one thread), each direction only while something wants it: the output while a road listens or
//! a sound plays, the input while a press is live or the meter is shown. Each want is a deadline
//! its user renews, so a direction nobody renews closes by itself. A named device that is not
//! there is said so, never swapped for the system default.
pub use tempo_codec::headset::*;

#[cfg(feature = "device")]
pub use device_headset::Headset;

#[cfg(feature = "device")]
mod device_headset {
    use super::{Cushion, Rings};
    use crate::device::{
        capture_config, dispatch_format, format_quality_rank, open_capture_with_fallback,
        resolve_configured, supported_device_format, DeviceSample, AUDIO_HOST_LOCK,
    };
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{SampleFormat, Stream};
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    /// How often the headset's thread looks at what is wanted.
    const LOOK: Duration = Duration::from_millis(20);
    /// How long a direction that would not open waits before it is tried again on the same device.
    const AGAIN: Duration = Duration::from_secs(2);

    struct Shared {
        rings: Arc<Rings>,
        /// The output and input chosen, "" for the system default.
        devices: Mutex<(String, String)>,
        /// Until when each direction is wanted, in ms from `epoch`.
        out_until: AtomicU64,
        in_until: AtomicU64,
        epoch: Instant,
        /// Why each direction is not open, when it would not open.
        trouble: Mutex<(Option<String>, Option<String>)>,
        /// Which directions are open now.
        open: Mutex<(bool, bool)>,
        stopping: AtomicBool,
    }

    impl Shared {
        fn ms(&self, at: Instant) -> u64 {
            u64::try_from(at.saturating_duration_since(self.epoch).as_millis()).unwrap_or(u64::MAX)
        }
    }

    /// This computer's headset: the rings, and the thread that opens and holds the devices while
    /// they are wanted. Cheap to share; the thread ends when the last holder lets it go.
    pub struct Headset {
        shared: Arc<Shared>,
    }

    impl Headset {
        /// The headset on `output` and `input` ("" for the system default), opening nothing until
        /// something wants a direction. `None` if its thread would not start.
        pub fn start(output: &str, input: &str) -> Option<Self> {
            let shared = Arc::new(Shared {
                rings: Arc::new(Rings::default()),
                devices: Mutex::new((output.to_string(), input.to_string())),
                out_until: AtomicU64::new(0),
                in_until: AtomicU64::new(0),
                epoch: Instant::now(),
                trouble: Mutex::new((None, None)),
                open: Mutex::new((false, false)),
                stopping: AtomicBool::new(false),
            });
            let theirs = shared.clone();
            std::thread::Builder::new()
                .name("nexus-headset".into())
                .spawn(move || run(&theirs))
                .ok()?;
            Some(Self { shared })
        }

        pub fn rings(&self) -> &Arc<Rings> {
            &self.shared.rings
        }

        /// The devices to use from now on; a direction open on another one opens again.
        pub fn devices(&self, output: &str, input: &str) {
            *self
                .shared
                .devices
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = (output.to_string(), input.to_string());
        }

        /// The output is wanted for `for_` from now: a road listening renews it, as does a sound.
        pub fn want_output(&self, for_: Duration) {
            let until = self.shared.ms(Instant::now() + for_);
            self.shared.out_until.fetch_max(until, Ordering::AcqRel);
        }

        /// The input is wanted for `for_` from now: a live press renews it, as does the meter.
        pub fn want_input(&self, for_: Duration) {
            let until = self.shared.ms(Instant::now() + for_);
            self.shared.in_until.fetch_max(until, Ordering::AcqRel);
        }

        /// Why the output and the input are not open, for the ones that would not open.
        pub fn trouble(&self) -> (Option<String>, Option<String>) {
            self.shared
                .trouble
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone()
        }

        /// Whether the output and the input are open now.
        pub fn open(&self) -> (bool, bool) {
            *self.shared.open.lock().unwrap_or_else(|e| e.into_inner())
        }
    }

    impl Drop for Headset {
        fn drop(&mut self) {
            self.shared.stopping.store(true, Ordering::Release);
        }
    }

    /// One direction as the thread holds it.
    #[derive(Default)]
    struct Held {
        stream: Option<Stream>,
        /// The device it is open on.
        device: String,
        /// The device that last would not open, and when.
        failed: Option<(String, Instant)>,
    }

    fn run(shared: &Arc<Shared>) {
        let (mut out, mut input) = (Held::default(), Held::default());
        while !shared.stopping.load(Ordering::Acquire) {
            let now = Instant::now();
            let ms = shared.ms(now);
            let (want_out, want_in) = shared
                .devices
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            let out_wanted = ms < shared.out_until.load(Ordering::Acquire);
            let in_wanted = ms < shared.in_until.load(Ordering::Acquire);
            let out_trouble = keep(&mut out, out_wanted, &want_out, now, |device| {
                open_output(device, &shared.rings)
            });
            let in_trouble = keep(&mut input, in_wanted, &want_in, now, |device| {
                open_input(device, &shared.rings)
            });
            if out.stream.is_none() {
                shared.rings.out_rate.store(0, Ordering::Release);
            }
            if input.stream.is_none() {
                shared.rings.in_rate.store(0, Ordering::Release);
            }
            {
                let mut trouble = shared.trouble.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(said) = out_trouble {
                    trouble.0 = said;
                }
                if let Some(said) = in_trouble {
                    trouble.1 = said;
                }
            }
            *shared.open.lock().unwrap_or_else(|e| e.into_inner()) =
                (out.stream.is_some(), input.stream.is_some());
            std::thread::sleep(LOOK);
        }
        // The streams close on this thread, where they opened, and under the host lock.
        let _guard = AUDIO_HOST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        drop(out);
        drop(input);
    }

    /// One direction kept as wanted: closed when not wanted, opened (again) on `device` when it is
    /// and is not open there. What changed in why it is not open, if anything.
    fn keep(
        held: &mut Held,
        wanted: bool,
        device: &str,
        now: Instant,
        open: impl FnOnce(&str) -> Result<(Stream, u32), String>,
    ) -> Option<Option<String>> {
        if !wanted {
            if held.stream.is_some() {
                let _guard = AUDIO_HOST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
                held.stream = None;
            }
            return None;
        }
        if held.stream.is_some() && held.device == device {
            return None;
        }
        let recently = held
            .failed
            .as_ref()
            .is_some_and(|(on, at)| on == device && now.saturating_duration_since(*at) < AGAIN);
        if recently {
            return None;
        }
        let _guard = AUDIO_HOST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        held.stream = None;
        match open(device) {
            Ok((stream, _)) => {
                held.stream = Some(stream);
                held.device = device.to_string();
                held.failed = None;
                Some(None)
            }
            Err(why) => {
                held.failed = Some((device.to_string(), now));
                Some(Some(why))
            }
        }
    }

    /// The best buildable config at `rate`, if the device offers one.
    fn config_at(
        configs: impl Iterator<Item = cpal::SupportedStreamConfigRange>,
        rate: u32,
    ) -> Option<cpal::SupportedStreamConfig> {
        configs
            .filter(|range| {
                supported_device_format(range.sample_format())
                    && range.min_sample_rate() <= rate
                    && rate <= range.max_sample_rate()
            })
            .max_by_key(|range| format_quality_rank(range.sample_format()))
            .and_then(|range| range.try_with_sample_rate(rate))
    }

    /// The output on `device` ("" for the default), at 48 kHz where it offers it: the stream and
    /// its rate. The caller holds the host lock.
    fn open_output(device: &str, rings: &Arc<Rings>) -> Result<(Stream, u32), String> {
        let host = cpal::default_host();
        let name = (!device.trim().is_empty()).then_some(device);
        let dev = resolve_configured(
            || host.output_devices().ok(),
            name,
            host.default_output_device(),
            "headset output",
        )?;
        let supported = dev
            .supported_output_configs()
            .ok()
            .and_then(|configs| config_at(configs, crate::receive_decode::PLAY_RATE_HZ))
            .or_else(|| dev.default_output_config().ok())
            .ok_or("this output offers no way to play")?;
        let rate = supported.sample_rate();
        let channels = (supported.channels() as usize).max(1);
        let config = supported.config();
        let stream = dispatch_format!(
            supported.sample_format(),
            build_output,
            "headset output",
            &dev,
            &config,
            channels,
            rate,
            rings,
        )?;
        stream.play().map_err(|e| e.to_string())?;
        rings.out_rate.store(rate, Ordering::Release);
        rings.opened.fetch_add(1, Ordering::AcqRel);
        Ok((stream, rate))
    }

    /// The output callback. REALTIME: the cushion and the rings only.
    fn build_output<T: DeviceSample + Send + 'static>(
        dev: &cpal::Device,
        config: &cpal::StreamConfig,
        channels: usize,
        rate: u32,
        rings: &Arc<Rings>,
    ) -> Result<Stream, String> {
        let rings = rings.clone();
        let mut cushion = Cushion::new(rate);
        dev.build_output_stream(
            *config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                cushion.begin(&rings);
                for frame in data.chunks_mut(channels) {
                    let sample = T::from_unit(cushion.next(&rings));
                    for slot in frame.iter_mut() {
                        *slot = sample;
                    }
                }
            },
            |e| eprintln!("tempo-audio: headset output error: {e}"),
            None,
        )
        .map_err(|e| e.to_string())
    }

    /// The input on `device` ("" for the default), at its own rate: the stream and its rate. The
    /// caller holds the host lock.
    fn open_input(device: &str, rings: &Arc<Rings>) -> Result<(Stream, u32), String> {
        let host = cpal::default_host();
        let name = (!device.trim().is_empty()).then_some(device);
        let dev = resolve_configured(
            || host.input_devices().ok(),
            name,
            host.default_input_device(),
            "headset microphone",
        )?;
        let supported = dev.default_input_config().map_err(|e| e.to_string())?;
        let rate = supported.sample_rate();
        let channels = (supported.channels() as usize).max(1);
        let format: SampleFormat = supported.sample_format();
        let stream = open_capture_with_fallback(capture_config(&supported), |config| {
            dispatch_format!(
                format,
                build_input,
                "headset microphone",
                &dev,
                config,
                channels,
                rings,
            )
        })?;
        stream.play().map_err(|e| e.to_string())?;
        rings.in_rate.store(rate, Ordering::Release);
        rings.opened.fetch_add(1, Ordering::AcqRel);
        Ok((stream, rate))
    }

    /// The input callback, folding each frame to mono by averaging. REALTIME: the ring only.
    fn build_input<T: DeviceSample + Send + 'static>(
        dev: &cpal::Device,
        config: cpal::StreamConfig,
        channels: usize,
        rings: &Arc<Rings>,
    ) -> Result<Stream, String> {
        let rings = rings.clone();
        dev.build_input_stream(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                rings.captured(data.chunks(channels).map(|frame| {
                    frame.iter().map(|&s| s.to_unit()).sum::<f32>() / channels as f32
                }));
            },
            |e| eprintln!("tempo-audio: headset microphone error: {e}"),
            None,
        )
        .map_err(|e| e.to_string())
    }
}
