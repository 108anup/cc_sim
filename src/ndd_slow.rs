use std::cell::RefCell;
use std::collections::VecDeque;
use std::fmt::Display;
use std::rc::Rc;

use crate::metrics::{CsvMetric, MetricRegistry};
use crate::rtt_window::RTTWindow;
use crate::simulator::{PktId, SeqNum, Time};
use crate::transport::CongestionControl;

pub const MIN_CWND: f64 = 8.;
pub const MIN_INTERSEND_TIME: Time = Time::from_micros(1000); // TODO: correct value
pub const PROBE_GAIN: f64 = 1.25;
pub const MULTIPLIER: f64 = 1.125;

pub const CRUISE_STEPS: u32 = 10;
pub const PROBE_STEPS: u32 = 2;
pub const DRAIN_STEPS: u32 = 1;
pub const CYCLE_STEPS: u32 = PROBE_STEPS + DRAIN_STEPS + CRUISE_STEPS;
// Currently a step is RTT.
// TODO: Ideally it should be global absolute shared constant.

fn option_to_string<T: Display>(x: Option<T>) -> String {
    match x {
        Some(x) => x.to_string(),
        None => "nan".to_string(),
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
enum NDDFSMAction {
    SlowStart,
    FirstCruise,
    Cruise,
    FirstProbe,
    Probe,
    Drain,
}

fn get_action_from_phase(_phase: u32) -> NDDFSMAction {
    let phase = _phase % CYCLE_STEPS;
    if phase == 0 {
        NDDFSMAction::FirstCruise
    } else if phase < CRUISE_STEPS {
        NDDFSMAction::Cruise
    } else if phase == CRUISE_STEPS {
        NDDFSMAction::FirstProbe
    } else if phase < CRUISE_STEPS + PROBE_STEPS {
        NDDFSMAction::Probe
    } else {
        NDDFSMAction::Drain
    }
}

pub const N_RECORDS: usize = CYCLE_STEPS as usize;

struct Record {
    snd_beg_seq: SeqNum,
    snd_beg_time: Time,
    snd_end_seq: Option<SeqNum>,
    snd_end_time: Option<Time>,

    ack_beg_seq: Option<SeqNum>,
    ack_beg_time: Option<Time>,
    ack_end_seq: Option<SeqNum>,
    ack_end_time: Option<Time>,

    snd_complete: bool,
    ack_complete: bool,

    cwnd: f64, // We only change cwnd at beginning of new Record?
    phase: u32,
    action: NDDFSMAction,
}

// Convention: we change cwnd at the boundary of a record. Every new record
// starts with a new cwnd value and this value does not change over the life of
// the record.

impl Record {
    fn snd_rate(&self) -> f64 // packets per sec
    {
        let dur = self.snd_end_time.unwrap() - self.snd_beg_time;
        let dur_secs = dur.secs();
        let num_pkts = self.snd_end_seq.unwrap() - self.snd_beg_seq;
        num_pkts as f64 / dur_secs
    }

    fn ack_rate(&self) -> f64 // packets per sec
    {
        let dur = self.ack_end_time.unwrap() - self.ack_beg_time.unwrap();
        let dur_secs = dur.secs();
        let num_pkts = self.ack_end_seq.unwrap() - self.ack_beg_seq.unwrap();
        num_pkts as f64 / dur_secs
    }

    fn c_estimate(&self, f_estimate: f64) -> f64 {
        let snd_rate = self.snd_rate();
        let ack_rate = self.ack_rate();

        assert!(snd_rate > ack_rate);
        // assert!(ack_rate >= f_estimate);

        (snd_rate * ack_rate - f_estimate * ack_rate) / (snd_rate - ack_rate)
    }

    fn n_estimate(&self, f_estimate: f64) -> f64 {
        self.c_estimate(f_estimate) / f_estimate
    }

    fn acked_pkts(&self) -> u64 {
        (self.ack_end_seq.unwrap() - self.ack_beg_seq.unwrap()) as u64
    }

    fn ack_duration(&self) -> f64 {
        let dur = self.ack_end_time.unwrap() - self.ack_beg_time.unwrap();
        dur.secs()
    }
}

pub struct NDDSlow {
    base_rtt: RTTWindow,
    min_rtt: Time,

    records: VecDeque<Record>,
    cwnd: f64,
    prev_cwnd: f64,
    cruise_cwnd: f64,
    phase: u32,
    action: NDDFSMAction,

    f_estimate: f64,
    n_estimate: f64,
    target_delay: f64,

    metric_registry: Option<MetricRegistry>,
    ack_metric: Option<Rc<RefCell<CsvMetric>>>,
}

pub struct NDDAckMetric {
    now: Time,
    cwnd: f64,
    phase: u32,
    action: NDDFSMAction,
    f_estimate: f64,
    n_estimate: f64,
    target_delay: f64,
    srtt: Time,
    min_rtt: Time,
    average_delay: f64,
    inst_delay: Time,
}

impl NDDAckMetric {
    fn to_row(&self) -> Vec<String> {
        vec![
            self.now.to_string(),
            self.cwnd.to_string(),
            self.phase.to_string(),
            format!("{:?}", self.action),
            self.f_estimate.to_string(),
            self.n_estimate.to_string(),
            self.target_delay.to_string(),
            self.srtt.to_string(),
            self.min_rtt.to_string(),
            self.average_delay.to_string(),
            self.inst_delay.to_string(),
        ]
    }

    fn get_columns() -> Vec<String> {
        vec![
            "now".to_string(),
            "cwnd".to_string(),
            "phase".to_string(),
            "action".to_string(),
            "f_estimate".to_string(),
            "n_estimate".to_string(),
            "target_delay".to_string(),
            "srtt".to_string(),
            "min_rtt".to_string(),
            "average_delay".to_string(),
            "inst_delay".to_string(),
        ]
    }
}

impl Default for NDDSlow {
    fn default() -> Self {
        Self {
            base_rtt: RTTWindow::new(Time::from_secs(10)),
            min_rtt: Time::from_secs(10),
            records: VecDeque::new(),
            cwnd: MIN_CWND,
            prev_cwnd: MIN_CWND,
            cruise_cwnd: MIN_CWND,
            phase: 0,
            action: NDDFSMAction::SlowStart,
            f_estimate: 0.,
            n_estimate: 1.,
            target_delay: Time::from_secs(10).secs(),
            metric_registry: None,
            ack_metric: None,
        }
    }
}

impl CongestionControl for NDDSlow {
    fn on_ack(&mut self, now: Time, _cum_ack: SeqNum, _ack_uid: PktId, rtt: Time, num_lost: u64) {
        if num_lost > 0 {
            // TODO: implement response for loss
        }

        // ---------------------------------------------------------------------
        // Update RTT state
        self.base_rtt.new_rtt_sample(rtt, now);
        // Update the windows over which we compute history
        self.base_rtt.change_hist_period(
            std::cmp::max(
                Time::from_secs(10),
                Time::from_micros(30 * self.base_rtt.get_srtt().micros()),
            ),
            now,
        );
        self.min_rtt = self.min_rtt.min(rtt);
        let average_delay = (self.base_rtt.get_srtt() - self.min_rtt).secs();
        let inst_delay = rtt - self.min_rtt;

        // ---------------------------------------------------------------------
        // Update records
        let mut update_cwnd = false;
        let last_record = self.records.back_mut().unwrap(); // we can only get ack if we sent something so this should always be Some
        if _cum_ack - 1 == last_record.snd_beg_seq {
            // TODO: we should do the right thing even if the equality checks
            // don't go through.
            last_record.snd_complete = true;
            last_record.ack_beg_seq = Some(_cum_ack - 1);
            last_record.ack_beg_time = Some(now);
            update_cwnd = true;
        } else if _cum_ack - 1 < last_record.snd_beg_seq {
            // There must be a record before last one whose ACKs have not
            // completed
            let slr = self.records.iter_mut().rev().nth(1).unwrap(); // second last record
            if _cum_ack - 1 == slr.snd_end_seq.unwrap() {
                slr.ack_complete = true;
                slr.ack_end_seq = Some(_cum_ack - 1);
                slr.ack_end_time = Some(now);
            } else {
                // We are still getting ACKs for the second last record, it is
                // not yet complete.
            }
        }

        while self.records.len() > N_RECORDS {
            self.records.pop_front();
        }

        // ---------------------------------------------------------------------
        // Update cwnd
        if update_cwnd {
            self.prev_cwnd = self.cwnd;
            self.phase = (self.phase + 1) % CYCLE_STEPS;
            self.action = get_action_from_phase(self.phase);
            let last_complete_record = self.records.iter().rev().find(|r| r.ack_complete);
            if last_complete_record.is_none() {
                self.action = NDDFSMAction::SlowStart;
            }

            // Update cwnd based on action
            // FirstCruise, set based on estimated N compared with delay / D.
            // Probe, increase by 1.25x.
            // Drain reset to cwnd before probe.
            // Cruise no change.

            match self.action {
                NDDFSMAction::SlowStart => {
                    // TODO: Currently doing MIMD on delay. Ideally replace
                    // with something like BBR (increase until saturation).
                    if average_delay <= 1.5 * self.min_rtt.secs() {
                        self.cwnd *= 2.;
                    } else {
                        self.cwnd *= 1. / 2.;
                    }
                    self.cruise_cwnd = self.cwnd;
                }
                NDDFSMAction::FirstCruise => {
                    // complete record that happened in the probing phase gives us an
                    // estimate of the bottleneck bandwidth.
                    let opt_last_complete_probe = self
                        .records
                        .iter()
                        .rev()
                        .find(|r| r.ack_complete && r.action == NDDFSMAction::Probe);

                    // NOTE: it may be that the last complete record is slow
                    // start. We could try to find a record for which sending
                    // rate more than ACK rate.

                    // DEPRECATED: If a complete record exists, and we are in
                    // FirstCruise state now, then there must be a complete
                    // probe record.

                    if let Some(last_complete_probe) = opt_last_complete_probe {
                        // F = average ACK rate in all the cruise states
                        let mut f_estimate_num = 0;
                        let mut f_estimate_den = 0.;
                        let mut count = 0;
                        for r in self.records.iter().rev() {
                            if r.ack_complete && r.action == NDDFSMAction::Cruise {
                                f_estimate_num += r.acked_pkts();
                                f_estimate_den += r.ack_duration();
                            }
                            count += 1;
                            if count > CRUISE_STEPS / 2 {
                                break;
                            }
                        }
                        self.f_estimate = f_estimate_num as f64 / f_estimate_den;
                        self.n_estimate =
                            if last_complete_probe.ack_rate() >= last_complete_probe.snd_rate() {
                                1.
                            } else {
                                last_complete_probe.n_estimate(self.f_estimate)
                            };
                        self.target_delay = self.n_estimate * self.min_rtt.secs();

                        // TODO: Damp the multiplier based on gap between
                        // target and actual delay.
                        let target_cwnd = self.cwnd * self.target_delay / average_delay;
                        let max_cwnd = MULTIPLIER * self.cwnd;
                        let min_cwnd = self.cwnd / MULTIPLIER;
                        let mean_cwnd = (self.cwnd + target_cwnd) / 2.;
                        self.cwnd = f64::max(min_cwnd, f64::min(max_cwnd, mean_cwnd));
                        // if self.target_delay > average_delay {
                        //     self.cwnd *= MULTIPLIER;
                        // } else {
                        //     self.cwnd *= 1. / MULTIPLIER;
                        // }
                    } else {
                        // If we don't have a complete probe record, then we
                        // don't have an estimate of the bottleneck bandwidth.
                        // Emulate regular Cruise in this case.
                    }
                    self.cruise_cwnd = self.cwnd;
                }
                NDDFSMAction::Cruise => {
                    // Do nothing
                }
                NDDFSMAction::FirstProbe => {
                    self.cwnd *= PROBE_GAIN;
                }
                NDDFSMAction::Probe => {
                    // Do nothing
                }
                NDDFSMAction::Drain => {
                    self.cwnd = self.cruise_cwnd;
                }
            }
            self.cwnd = f64::max(MIN_CWND, self.cwnd);
            // TODO: add upper limit on cwnd

            // ---------------------------------------------------------------------
            // Log metrics (for debugging)
            self.ack_metric.as_ref().unwrap().borrow_mut().log(
                NDDAckMetric {
                    now,
                    cwnd: self.cwnd,
                    phase: self.phase,
                    action: self.action,
                    f_estimate: self.f_estimate,
                    n_estimate: self.n_estimate,
                    target_delay: self.target_delay,
                    srtt: self.base_rtt.get_srtt(),
                    min_rtt: self.min_rtt,
                    average_delay,
                    inst_delay,
                }
                .to_row(),
            );
        }
    }

    fn on_send(&mut self, _now: Time, _seq_num: SeqNum, _uid: PktId) {
        let last_record = self.records.back_mut();
        if last_record.is_none() || last_record.unwrap().snd_complete {
            self.records.push_back(Record {
                snd_beg_seq: _seq_num,
                snd_beg_time: _now,
                snd_end_seq: None,
                snd_end_time: None,
                ack_beg_seq: None,
                ack_beg_time: None,
                ack_end_seq: None,
                ack_end_time: None,
                snd_complete: false,
                ack_complete: false,
                cwnd: self.cwnd,
                phase: self.phase,
                action: self.action,
            });
        } else {
            let last_record = self.records.back_mut().unwrap();
            // Keep updating the record (we will close it when we get ack for
            // the snd_beg_seq)
            last_record.snd_end_seq = Some(_seq_num);
            last_record.snd_end_time = Some(_now);
        }
    }

    fn on_timeout(&mut self) {
        self.cwnd = 2.;
    }

    fn get_cwnd(&mut self) -> u64 {
        (self.cwnd).round() as u64
    }

    fn get_intersend_time(&mut self) -> Time {
        std::cmp::max(
            MIN_INTERSEND_TIME,
            Time::from_micros((2e6 * self.base_rtt.get_srtt().secs() / self.cwnd) as u64),
        )
    }

    fn init(&mut self, name: &str, metrics_config_file: Option<String>) {
        if let Some(metrics_config_file) = metrics_config_file {
            self.metric_registry = Some(MetricRegistry::new(&metrics_config_file));
            let metric_name: &str = &(name.to_owned() + "ack");
            self.ack_metric = self
                .metric_registry
                .as_mut()
                .unwrap()
                .register_csv_metric(metric_name, NDDAckMetric::get_columns());
        }
        // TODO: replace with pseudo-random number generator with a controlled
        // seed for reproducing results. Each flow should get a different
        // random number though.
        self.phase = rand::random::<u32>() % CYCLE_STEPS;
    }

    fn finish(&self) {
        if let Some(metric_registry) = &self.metric_registry {
            metric_registry.finish();
        }
    }
}
