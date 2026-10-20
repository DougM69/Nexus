//! The radio as the server reports it, folded from its notices (TCI 2.0 §4.1–§4.5).
//!
//! The server is a synchronizer (§3.1, §3.5): every change, by any client or by the SDR's own
//! operator, is broadcast to every client, so this model is the server's word on its own state.
//! Nothing else writes it: Nexus's own writes reach it only through the server's echo, so a write
//! the server ignored (its 200 ms hold, a band restore) never shows as done.
//!
//! **A refused readback is unknown, not unchanged.** Every parameter is a [`Reading`]: never
//! reported, a value, or [`Reading::Unreadable`] when the server's last report of it could not be
//! read. A command that fails to decode carries no receiver number that can be trusted, so the
//! parameter it names becomes unreadable on every receiver ([`Model::refused`]) until a report
//! that reads replaces it. Admission refuses a key on an unreadable parameter it relies on
//! ([`super::admission`]), and the session asks again for what a query can ask
//! ([`Model::rereads`]).
//!
//! Bounded: receiver and channel numbers are bounded by their types ([`Trx`], [`Channel`]), so
//! the model is a fixed [`MAX_TRX`] × [`MAX_CHANNELS`].

use super::wire::{
    Flag, Hz, Modes, Notice, Query, Real, Setting, Source, StreamSetting, Token, Word,
};
use super::{Channel, Dialect, Trx, MAX_CHANNELS, MAX_TRX};

/// One parameter as the server last reported it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Reading<T> {
    /// The server has not reported it on this connection.
    #[default]
    NotReported,
    Known(T),
    /// The server's last report of it could not be read: unknown, never the value before.
    Unreadable,
}

impl<T> Reading<T> {
    /// The value, when one is known.
    pub fn known(&self) -> Option<&T> {
        match self {
            Reading::Known(v) => Some(v),
            Reading::NotReported | Reading::Unreadable => None,
        }
    }

    pub fn is_unreadable(&self) -> bool {
        matches!(self, Reading::Unreadable)
    }
}

impl Reading<bool> {
    /// Known to be `true`.
    pub fn is_true(&self) -> bool {
        matches!(self, Reading::Known(true))
    }

    /// Known to be `false`.
    pub fn is_false(&self) -> bool {
        matches!(self, Reading::Known(false))
    }
}

const TRXS: usize = MAX_TRX as usize;
const CHANNELS: usize = MAX_CHANNELS as usize;

/// The transmitter's readings (`tx_sensors`, §4.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TxSensors {
    pub mic_dbm: Real,
    pub rms_w: Real,
    pub peak_w: Real,
    pub swr: Real,
}

/// One receiver (transceiver) as the server reports it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrxModel {
    /// The panorama's centre.
    pub dds: Reading<Hz>,
    /// Each channel's (VFO's) frequency: 0 is A, 1 is B.
    pub vfo: [Reading<Hz>; CHANNELS],
    /// Each channel's IF offset within the panorama.
    pub if_hz: [Reading<Hz>; CHANNELS],
    pub modulation: Reading<Word>,
    /// The receive filter's edges, relative to the carrier.
    pub filter: Reading<(Hz, Hz)>,
    /// The drive as last reported in either form: `drive:<trx>,<v>;`, or the 1.x `drive:<v>;`,
    /// which sets every receiver. The newer report wins, so an older one never hides a newer one.
    pub drive: Reading<u32>,
    pub tune_drive: Reading<u32>,
    pub split: Reading<bool>,
    pub rit: Reading<bool>,
    pub rit_offset: Reading<Hz>,
    pub xit: Reading<bool>,
    pub xit_offset: Reading<Hz>,
    /// Whether this transmitter may transmit (§4.3; at connect and on a band change).
    pub tx_enable: Reading<bool>,
    /// `trx`: transmitting, by anyone (another program and the SDR's own operator included).
    pub transmitting: Reading<bool>,
    /// The source the server named with its last `trx:<trx>,true`, when it named one.
    pub source: Option<Source>,
    /// `tune`: the tune carrier is up.
    pub tuning: Reading<bool>,
    /// The receive level per channel, dBm: `rx_channel_sensors`, `rx_smeter`, or `rx_sensors`
    /// (which has no channel and is read as channel A).
    pub rx_dbm: [Reading<Real>; CHANNELS],
    pub tx_sensors: Reading<TxSensors>,
    pub vfo_lock: [Reading<bool>; CHANNELS],
    /// This client's receive audio stream: `audio_start:<trx>;` acknowledged (true) or
    /// `audio_stop:<trx>;` (false).
    pub audio: Reading<bool>,
    /// This client's IQ stream, the same way.
    pub iq: Reading<bool>,
}

/// Everything the server has reported on this connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Model {
    pub vfo_limits: Reading<(Hz, Hz)>,
    pub if_limits: Reading<(Hz, Hz)>,
    pub trx_count: Reading<u32>,
    pub channel_count: Reading<u32>,
    pub device: Reading<Token>,
    pub receive_only: Reading<bool>,
    pub modulations: Reading<Modes>,
    /// The program and the TCI version it speaks. Not an identity ([`super::dialect`]).
    pub protocol: Reading<(Token, Token)>,
    /// `ready;` has arrived.
    pub ready: bool,
    /// The SDR program runs (`start;`) or is stopped (`stop;`).
    pub running: Reading<bool>,
    pub tx_frequency: Reading<Hz>,
    /// `tx_power` and `tx_swr` (not in the spec; older servers).
    pub tx_power: Reading<Real>,
    pub tx_swr: Reading<Real>,
    trx: [TrxModel; TRXS],
}

impl Default for Model {
    fn default() -> Model {
        Model {
            vfo_limits: Reading::NotReported,
            if_limits: Reading::NotReported,
            trx_count: Reading::NotReported,
            channel_count: Reading::NotReported,
            device: Reading::NotReported,
            receive_only: Reading::NotReported,
            modulations: Reading::NotReported,
            protocol: Reading::NotReported,
            ready: false,
            running: Reading::NotReported,
            tx_frequency: Reading::NotReported,
            tx_power: Reading::NotReported,
            tx_swr: Reading::NotReported,
            trx: std::array::from_fn(|_| TrxModel::default()),
        }
    }
}

impl Model {
    pub fn new() -> Model {
        Model::default()
    }

    /// One receiver.
    pub fn trx(&self, trx: Trx) -> &TrxModel {
        &self.trx[trx.index()]
    }

    fn trx_mut(&mut self, trx: Trx) -> &mut TrxModel {
        &mut self.trx[trx.index()]
    }

    /// A receiver's drive, as last reported in either form ([`TrxModel::drive`]).
    pub fn drive(&self, trx: Trx) -> &Reading<u32> {
        &self.trx(trx).drive
    }

    /// Fold one notice in.
    pub fn apply(&mut self, notice: &Notice) {
        match notice {
            Notice::VfoLimits { low, high } => self.vfo_limits = Reading::Known((*low, *high)),
            Notice::IfLimits { low, high } => self.if_limits = Reading::Known((*low, *high)),
            Notice::TrxCount(n) => self.trx_count = Reading::Known(*n),
            Notice::ChannelCount(n) => self.channel_count = Reading::Known(*n),
            Notice::Device(name) => self.device = Reading::Known(name.clone()),
            Notice::ReceiveOnly(on) => self.receive_only = Reading::Known(*on),
            Notice::ModulationsList(modes) => self.modulations = Reading::Known(modes.clone()),
            Notice::Protocol { program, version } => {
                self.protocol = Reading::Known((program.clone(), version.clone()));
            }
            Notice::Ready => self.ready = true,
            Notice::RadioStart => self.running = Reading::Known(true),
            Notice::RadioStop => self.running = Reading::Known(false),
            Notice::State(setting) => self.setting(setting),
            Notice::Trx { trx, on, source } => {
                let t = self.trx_mut(*trx);
                t.transmitting = Reading::Known(*on);
                t.source = if *on { source.clone() } else { None };
            }
            Notice::Tune { trx, on } => self.trx_mut(*trx).tuning = Reading::Known(*on),
            Notice::TxEnable { trx, on } => self.trx_mut(*trx).tx_enable = Reading::Known(*on),
            Notice::TxFrequency(hz) => self.tx_frequency = Reading::Known(*hz),
            Notice::VfoLock { trx, channel, on } => {
                self.trx_mut(*trx).vfo_lock[channel.index()] = Reading::Known(*on);
            }
            Notice::Stream(stream) => self.stream(stream),
            Notice::RxSensors { trx, dbm } => {
                self.trx_mut(*trx).rx_dbm[Channel::A.index()] = Reading::Known(*dbm);
            }
            Notice::RxChannelSensors { trx, channel, dbm }
            | Notice::RxSmeter { trx, channel, dbm } => {
                self.trx_mut(*trx).rx_dbm[channel.index()] = Reading::Known(*dbm);
            }
            Notice::TxSensors {
                trx,
                mic_dbm,
                rms_w,
                peak_w,
                swr,
            } => {
                self.trx_mut(*trx).tx_sensors = Reading::Known(TxSensors {
                    mic_dbm: *mic_dbm,
                    rms_w: *rms_w,
                    peak_w: *peak_w,
                    swr: *swr,
                });
            }
            Notice::TxPower(w) => self.tx_power = Reading::Known(*w),
            Notice::TxSwr(swr) => self.tx_swr = Reading::Known(*swr),
            // Events for the session, or for later tasks (CW, spots); nothing to hold.
            Notice::ClickedOnSpot { .. }
            | Notice::RxClickedOnSpot { .. }
            | Notice::TxFootswitch { .. }
            | Notice::AppFocus(_)
            | Notice::CwMacrosEmpty
            | Notice::CallsignSend(_) => {}
        }
    }

    fn setting(&mut self, setting: &Setting) {
        match setting {
            Setting::Dds { trx, hz } => self.trx_mut(*trx).dds = Reading::Known(*hz),
            Setting::If { trx, channel, hz } => {
                self.trx_mut(*trx).if_hz[channel.index()] = Reading::Known(*hz);
            }
            Setting::Vfo { trx, channel, hz } => {
                self.trx_mut(*trx).vfo[channel.index()] = Reading::Known(*hz);
            }
            Setting::Modulation { trx, mode, .. } => {
                self.trx_mut(*trx).modulation = Reading::Known(mode.clone());
            }
            Setting::Drive {
                trx: Some(trx),
                level,
            } => self.trx_mut(*trx).drive = Reading::Known(*level),
            // The 1.x form sets every receiver's drive: newer than what each last reported.
            Setting::Drive { trx: None, level } => {
                for t in &mut self.trx {
                    t.drive = Reading::Known(*level);
                }
            }
            Setting::TuneDrive { trx, level } => {
                self.trx_mut(*trx).tune_drive = Reading::Known(*level);
            }
            Setting::Flag { flag, trx, on } => {
                let t = self.trx_mut(*trx);
                match flag {
                    Flag::Split => t.split = Reading::Known(*on),
                    Flag::Rit => t.rit = Reading::Known(*on),
                    Flag::Xit => t.xit = Reading::Known(*on),
                    _ => {}
                }
            }
            Setting::RitOffset { trx, hz } => self.trx_mut(*trx).rit_offset = Reading::Known(*hz),
            Setting::XitOffset { trx, hz } => self.trx_mut(*trx).xit_offset = Reading::Known(*hz),
            Setting::RxFilterBand { trx, low, high } => {
                self.trx_mut(*trx).filter = Reading::Known((*low, *high));
            }
            // Not needed by the session or the shim (yet): audio levels, AGC, noise blanker,
            // squelch, CW speeds, DIGL/DIGU offsets, channel B on or off.
            _ => {}
        }
    }

    fn stream(&mut self, stream: &StreamSetting) {
        match stream {
            StreamSetting::AudioStart(trx) => self.trx_mut(*trx).audio = Reading::Known(true),
            StreamSetting::AudioStop(trx) => self.trx_mut(*trx).audio = Reading::Known(false),
            StreamSetting::IqStart(trx) => self.trx_mut(*trx).iq = Reading::Known(true),
            StreamSetting::IqStop(trx) => self.trx_mut(*trx).iq = Reading::Known(false),
            _ => {}
        }
    }

    /// The server sent a `name` command that could not be read ([`super::wire::WireError::command`]):
    /// the parameter it names is unknown on every receiver until a report that reads replaces it.
    pub fn refused(&mut self, name: &str) {
        let global = match name {
            "receive_only" => Some(&mut self.receive_only),
            "start" | "stop" => Some(&mut self.running),
            _ => None,
        };
        if let Some(reading) = global {
            *reading = Reading::Unreadable;
            return;
        }
        match name {
            "tx_frequency" => self.tx_frequency = Reading::Unreadable,
            "tx_power" => self.tx_power = Reading::Unreadable,
            "tx_swr" => self.tx_swr = Reading::Unreadable,
            _ => {}
        }
        for t in &mut self.trx {
            match name {
                "dds" => t.dds = Reading::Unreadable,
                "vfo" => t.vfo = std::array::from_fn(|_| Reading::Unreadable),
                "if" => t.if_hz = std::array::from_fn(|_| Reading::Unreadable),
                "modulation" => t.modulation = Reading::Unreadable,
                "rx_filter_band" => t.filter = Reading::Unreadable,
                "drive" => t.drive = Reading::Unreadable,
                "tune_drive" => t.tune_drive = Reading::Unreadable,
                "split_enable" => t.split = Reading::Unreadable,
                "rit_enable" => t.rit = Reading::Unreadable,
                "rit_offset" => t.rit_offset = Reading::Unreadable,
                "xit_enable" => t.xit = Reading::Unreadable,
                "xit_offset" => t.xit_offset = Reading::Unreadable,
                "tx_enable" => t.tx_enable = Reading::Unreadable,
                "trx" => {
                    t.transmitting = Reading::Unreadable;
                    t.source = None;
                }
                "tune" => t.tuning = Reading::Unreadable,
                "rx_channel_sensors" | "rx_smeter" | "rx_sensors" => {
                    t.rx_dbm = std::array::from_fn(|_| Reading::Unreadable);
                }
                "tx_sensors" => t.tx_sensors = Reading::Unreadable,
                "vfo_lock" => t.vfo_lock = std::array::from_fn(|_| Reading::Unreadable),
                "audio_start" | "audio_stop" => t.audio = Reading::Unreadable,
                "iq_start" | "iq_stop" => t.iq = Reading::Unreadable,
                _ => {}
            }
        }
    }

    /// The reads that would replace what is unreadable on `trx`, in the form `dialect` reads.
    /// Pushed or one-off reports (`tx_enable`, `receive_only`, `start`/`stop`, the meters, the
    /// stream acknowledgements) have no read and wait for the server's next report.
    pub fn rereads(&self, trx: Trx, dialect: &Dialect) -> Vec<Query> {
        let t = self.trx(trx);
        let mut reads = Vec::new();
        if t.transmitting.is_unreadable() {
            reads.push(Query::Trx(trx));
        }
        if t.tuning.is_unreadable() {
            reads.push(Query::Tune(trx));
        }
        if self.drive(trx).is_unreadable() {
            reads.push(Query::drive(trx, dialect));
        }
        if t.tune_drive.is_unreadable() {
            reads.push(Query::TuneDrive(trx));
        }
        for channel in [Channel::A, Channel::B] {
            if t.vfo[channel.index()].is_unreadable() {
                reads.push(Query::Vfo { trx, channel });
            }
        }
        if t.modulation.is_unreadable() {
            reads.push(Query::Modulation(trx));
        }
        if t.filter.is_unreadable() {
            reads.push(Query::RxFilterBand(trx));
        }
        for (reading, flag) in [
            (&t.split, Flag::Split),
            (&t.rit, Flag::Rit),
            (&t.xit, Flag::Xit),
        ] {
            if reading.is_unreadable() {
                reads.push(Query::Flag { flag, trx });
            }
        }
        if t.rit_offset.is_unreadable() {
            reads.push(Query::RitOffset(trx));
        }
        if t.xit_offset.is_unreadable() {
            reads.push(Query::XitOffset(trx));
        }
        if t.dds.is_unreadable() {
            reads.push(Query::Dds(trx));
        }
        reads
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tci::wire::notices;
    use crate::tci::{EXPERT_SDR2, EXPERT_SDR3};

    fn trx(n: u32) -> Trx {
        Trx::new(n).expect("a receiver number")
    }

    /// Fold a server's text in; a command that does not read is refused by name, as the session
    /// does.
    fn fold(model: &mut Model, text: &str) {
        for item in notices(text) {
            match item {
                Ok(notice) => model.apply(&notice),
                Err(e) => {
                    if let Some(name) = e.command() {
                        model.refused(name);
                    }
                }
            }
        }
    }

    #[test]
    fn an_init_burst_folds_into_the_model() {
        let mut m = Model::new();
        fold(
            &mut m,
            "vfo_limits:10000,30000000;if_limits:-48000,48000;trx_count:2;channels_count:2;\
             device:SunSDR2DX;receive_only:false;modulations_list:AM,LSB,USB,CW,DIGU;\
             protocol:ExpertSDR3,2.0;dds:0,14080000;if:0,0,-6000;vfo:0,0,14074000;\
             vfo:0,1,14075000;modulation:0,USB;rx_filter_band:0,200,2900;rit_enable:0,false;\
             rit_offset:0,0;xit_enable:0,true;xit_offset:0,-50;split_enable:0,false;drive:0,50;\
             tune_drive:0,10;tx_enable:0,true;trx:0,false;tune:0,false;start;ready;",
        );
        assert!(m.ready);
        assert_eq!(m.trx_count, Reading::Known(2));
        assert!(m.receive_only.is_false());
        assert!(m.running.is_true());
        let t = m.trx(trx(0));
        assert_eq!(t.vfo[0].known().map(|h| h.get()), Some(14_074_000));
        assert_eq!(t.vfo[1].known().map(|h| h.get()), Some(14_075_000));
        assert_eq!(t.modulation.known().map(Word::as_str), Some("usb"));
        assert_eq!(m.drive(trx(0)), &Reading::Known(50));
        assert_eq!(t.tune_drive, Reading::Known(10));
        assert!(t.tx_enable.is_true() && t.transmitting.is_false() && t.tuning.is_false());
        assert!(t.xit.is_true());
        assert_eq!(t.xit_offset.known().map(|h| h.get()), Some(-50));
        assert_eq!(
            t.filter.known().map(|(l, h)| (l.get(), h.get())),
            Some((200, 2900))
        );
        // Nothing reported about receiver 1 beyond the count.
        assert_eq!(m.trx(trx(1)).transmitting, Reading::NotReported);
    }

    #[test]
    fn a_refused_readback_is_unknown_not_the_value_before() {
        let mut m = Model::new();
        fold(
            &mut m,
            "trx:0,false;drive:0,50;tx_enable:0,true;receive_only:false;start;vfo:0,0,7074000;",
        );
        // Garbage of exactly the kinds the simulator's Garbage fault sends, then the parameters
        // admission relies on that a server can only re-send.
        fold(
            &mut m,
            "trx:0,maybe;drive:0,-5;vfo:0,0,abc;tx_enable:0,perhaps;receive_only:x;stop:1;",
        );
        let t = m.trx(trx(0));
        assert!(t.transmitting.is_unreadable(), "trx");
        assert!(m.drive(trx(0)).is_unreadable(), "drive");
        assert!(t.vfo[0].is_unreadable(), "vfo");
        assert!(t.tx_enable.is_unreadable(), "tx_enable");
        assert!(m.receive_only.is_unreadable(), "receive_only");
        assert!(m.running.is_unreadable(), "start/stop");
        // Every receiver: the bad line's receiver number cannot be trusted.
        assert!(m.trx(trx(5)).transmitting.is_unreadable());
        // A report that reads replaces it.
        fold(&mut m, "trx:0,false;drive:0,50;");
        assert!(m.trx(trx(0)).transmitting.is_false());
        assert_eq!(m.drive(trx(0)), &Reading::Known(50));
    }

    #[test]
    fn what_is_unreadable_is_asked_again_where_a_read_exists() {
        let mut m = Model::new();
        fold(
            &mut m,
            "trx:0,x;tune:0,x;drive:0,x;tune_drive:0,x;vfo:0,0,x;modulation:0,;\
             rx_filter_band:0,x,1;split_enable:0,x;rit_enable:0,x;xit_enable:0,x;rit_offset:0,x;\
             xit_offset:0,x;dds:0,x;tx_enable:0,x;receive_only:x;tx_sensors:0,,,,;",
        );
        let lines: Vec<String> = m
            .rereads(trx(0), &EXPERT_SDR3)
            .iter()
            .map(|q| {
                crate::tci::wire::Command::Read(q.clone())
                    .render()
                    .into_string()
            })
            .collect();
        assert_eq!(
            lines,
            [
                "trx:0;",
                "tune:0;",
                "drive:0;",
                "tune_drive:0;",
                "vfo:0,0;",
                "vfo:0,1;",
                "modulation:0;",
                "rx_filter_band:0;",
                "split_enable:0;",
                "rit_enable:0;",
                "xit_enable:0;",
                "rit_offset:0;",
                "xit_offset:0;",
                "dds:0;",
            ]
        );
        // The 1.x drive read has no receiver.
        assert!(m
            .rereads(trx(0), &EXPERT_SDR2)
            .contains(&Query::Drive(None)));
        // Nothing to ask of a model that reads.
        assert!(Model::new().rereads(trx(0), &EXPERT_SDR3).is_empty());
    }

    #[test]
    fn the_drive_reads_either_form_and_a_receivers_own_wins() {
        let mut m = Model::new();
        fold(&mut m, "drive:35;");
        assert_eq!(m.drive(trx(0)), &Reading::Known(35));
        assert_eq!(m.drive(trx(1)), &Reading::Known(35));
        fold(&mut m, "drive:1,80;");
        assert_eq!(m.drive(trx(0)), &Reading::Known(35));
        assert_eq!(m.drive(trx(1)), &Reading::Known(80));
        // A level above 100 is held as reported: a readback above the ceiling must be seen.
        fold(&mut m, "drive:0,250;");
        assert_eq!(m.drive(trx(0)), &Reading::Known(250));
    }

    #[test]
    fn streams_meters_and_the_key_source_are_held() {
        let mut m = Model::new();
        fold(
            &mut m,
            "audio_start:0;iq_start:1;rx_channel_sensors:0,1,-90.5;rx_sensors:1,-70.0;\
             tx_sensors:0,-27.2,47.4,67.5,1.7;trx:0,true,mic1;",
        );
        let t = m.trx(trx(0));
        assert!(t.audio.is_true());
        assert!(m.trx(trx(1)).iq.is_true());
        assert_eq!(t.rx_dbm[1].known().map(|r| r.micros()), Some(-90_500_000));
        assert_eq!(
            m.trx(trx(1)).rx_dbm[0].known().map(|r| r.micros()),
            Some(-70_000_000)
        );
        assert_eq!(
            t.tx_sensors.known().map(|s| s.swr.micros()),
            Some(1_700_000)
        );
        assert_eq!(t.source, Some(Source::Mic1));
        fold(&mut m, "audio_stop:0;trx:0,false;");
        let t = m.trx(trx(0));
        assert!(t.audio.is_false());
        assert_eq!(t.source, None);
    }

    #[test]
    fn a_newer_report_of_the_drive_wins_whichever_form_it_takes() {
        // The review's finding: a receiver's older `drive:<trx>,<v>` hid a newer 1.x
        // `drive:<v>`, which sets every receiver.
        let mut m = Model::new();
        fold(&mut m, "drive:0,30;");
        fold(&mut m, "drive:100;");
        assert_eq!(
            m.drive(trx(0)),
            &Reading::Known(100),
            "the newer global report"
        );
        assert_eq!(m.drive(trx(1)), &Reading::Known(100));
        // And a receiver's own report after it is newer still, for that receiver alone.
        fold(&mut m, "drive:1,20;");
        assert_eq!(m.drive(trx(1)), &Reading::Known(20));
        assert_eq!(m.drive(trx(0)), &Reading::Known(100));
    }
}
