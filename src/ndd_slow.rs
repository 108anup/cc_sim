use std::any::Any;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::{Rc, Weak};

use circular_buffer::CircularBuffer;
use serde::{Deserialize, Serialize};

use crate::metrics::{CsvMetric, MetricRegistry};
use crate::rtt_window::RTTWindow;
use crate::simulator::{PktId, SeqNum, Time};
use crate::transport::CongestionControl;

pub const MIN_CWND: f64 = 8.;
pub const MIN_INTERSEND_TIME: Time = Time::from_micros(1000);  // TODO: correct value

pub const CYCLE_STEPS: u32 = 10;  // Currently a step is RTT, ideally it should be global absolute shared constant.
pub const PROBE_STEPS: u32 = 2;


enum NDDFSMState {
    Probe,
    Drain,
    Cruise
}

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

    cwnd: f64,  // We only change cwnd at beginning of new Record?
    state: NDDFSMState,
}

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
}

pub struct NDDSlow {
    base_rtt: RTTWindow,
    min_rtt: Time,

    records: VecDeque<Record>,
    cwnd: f64,
    prev_cwnd: f64,
    state: NDDFSMState,

    metric_registry: Option<MetricRegistry>,
    ack_metric: Option<Rc<RefCell<CsvMetric>>>,
}

impl Default for NDDSlow {
    fn default() -> Self {
        Self {
            base_rtt: RTTWindow::new(Time::from_secs(10)),
            min_rtt: Time::from_secs(10),
            records: VecDeque::new(),
            cwnd: MIN_CWND,
            prev_cwnd: MIN_CWND,
            state: NDDFSMState::Cruise,
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
        let delay = rtt - self.min_rtt;


        // Record
        let last_record = self.records.back_mut().unwrap();
        // ^^ We can only get ack if we sent something so this should always be
        // Some

        // TODO: we should do the right thing even if the equality checks don't
        // go through.
        if _cum_ack-1 == last_record.snd_beg_seq {
            last_record.snd_complete = true;
            last_record.ack_beg_seq = Some(_cum_ack);
            last_record.ack_beg_time = Some(now);
        } else if _cum_ack-1 < last_record.snd_beg_seq {
            // There must be a record before last one whose ACKs have not
            // completed
            // There must be a record before last one whose ACKs have not
            // completed
            let slr = self.records.iter_mut().rev().nth(1).unwrap(); // second last record
            if _cum_ack == slr.snd_end_seq.unwrap() {
            } else {
            }
        }

        while self.records.len() > 3 {
            self.records.pop_front();
        }

        let last_complete_record = self.records.iter().rev().find(|r| r.ack_complete);
        if last_complete_record.is_none() {
            // Slow start like stuff when we don't have last record. MIMD on
            // delay.
            if delay.secs() <= 1.5 * self.min_rtt.secs() {
                self.cwnd += 1. / 2.;
            } else {
                self.cwnd -= 1. / 2.;
            }
            self.cwnd = f64::max(MIN_CWND, self.cwnd);
            self.prev_cwnd = self.cwnd;
        }

        // TODO: add upper limit on cwnd
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
        std::cmp::max(MIN_INTERSEND_TIME, Time::from_micros((2e6 * self.base_rtt.get_srtt().secs() / self.cwnd) as u64))
    }

    fn init(&mut self, name: &str, metrics_config_file: Option<String>) {
        if let Some(metrics_config_file) = metrics_config_file {
            self.metric_registry = Some(MetricRegistry::new(&metrics_config_file));
            let metric_name: &str = &(name.to_owned() + "ack");
            self.ack_metric = self
                .metric_registry
                .as_mut()
                .unwrap()
                .register_csv_metric(metric_name, NDDState::get_columns());
        }
    }

    fn finish(&self) {
        if let Some(metric_registry) = &self.metric_registry {
            metric_registry.finish();
        }
    }
}