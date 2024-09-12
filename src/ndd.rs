// NDD: CCA that maintains O(N * D) delay when there are N flows.

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

pub const F_FILTER_LEN: usize = 1;
pub const MIN_CWND: f64 = 8.;
// since we have 25% increase/decrease, we want the increase to be more than a
// packet even when cwnd is lowest.
pub const MIN_INTERSEND_TIME: Time = Time::from_micros(1000);  // TODO: correct value

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

    n_min: Option<f64>,
    n_max: Option<f64>,
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
        assert!(ack_rate >= f_estimate);

        (snd_rate * ack_rate - f_estimate * ack_rate) / (snd_rate - ack_rate)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
struct NDDState {
    f_min: f64,
    f_max: f64,
    c_min: Option<f64>,
    c_max: Option<f64>,
    n_min: Option<f64>,
    n_max: Option<f64>,
    target_delay_min: Option<f64>,
    target_delay_max: Option<f64>,
    delay: f64,
    min_rtt: f64,
}

impl NDDState {
    fn option_to_string(x: Option<f64>) -> String {
        match x {
            Some(x) => x.to_string(),
            None => "nan".to_string(),
        }
    }

    fn to_row(&self) -> Vec<String> {
        vec![
            self.f_min.to_string(),
            self.f_max.to_string(),
            NDDState::option_to_string(self.c_min),
            NDDState::option_to_string(self.c_max),
            NDDState::option_to_string(self.n_min),
            NDDState::option_to_string(self.n_max),
            NDDState::option_to_string(self.target_delay_min),
            NDDState::option_to_string(self.target_delay_max),
            self.delay.to_string(),
            self.min_rtt.to_string(),
        ]
    }

    fn get_columns() -> Vec<String> {
        vec![
            "f_min".to_string(),
            "f_max".to_string(),
            "c_min".to_string(),
            "c_max".to_string(),
            "n_min".to_string(),
            "n_max".to_string(),
            "target_delay_min".to_string(),
            "target_delay_max".to_string(),
            "delay".to_string(),
            "min_rtt".to_string(),
        ]
    }
}

pub struct NDD {
    base_rtt: RTTWindow,
    min_rtt: Time,
    // TODO: ^^ We should add RTT probe to get running estimate of min_rtt.
    // Currently hack.

    f_min_estimates: CircularBuffer<F_FILTER_LEN, f64>,
    f_max_estimates: CircularBuffer<F_FILTER_LEN, f64>,
    records: VecDeque<Record>,
    cwnd: f64,
    prev_cwnd: f64,
    last_increase: bool,

    metric_registry: Option<MetricRegistry>,
    ack_metric: Option<Rc<RefCell<CsvMetric>>>,
}

impl Default for NDD {
    fn default() -> Self {
        // TODO: compute correct initial values
        let mut _fmin_estimates = CircularBuffer::new();
        _fmin_estimates.push_back(1.);  // pkt per sec
        let mut _fmax_estimates = CircularBuffer::new();
        _fmax_estimates.push_back(1e9);  // pkt per sec
        Self {
            base_rtt: RTTWindow::new(Time::from_secs(10)),
            min_rtt: Time::from_secs(10),  // TODO: correct initial value
            f_min_estimates: _fmin_estimates,
            f_max_estimates: _fmax_estimates,
            records: VecDeque::new(),
            cwnd: MIN_CWND,
            prev_cwnd: MIN_CWND,
            last_increase: false,

            metric_registry: None,
            ack_metric: None,
        }
    }
}

impl CongestionControl for NDD {
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

        self.min_rtt = std::cmp::min(self.min_rtt, rtt);
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
            let slr = self.records.iter_mut().rev().nth(1).unwrap(); // second last record
            if _cum_ack == slr.snd_end_seq.unwrap() {
                slr.ack_complete = true;
                slr.ack_end_seq = Some(_cum_ack);
                slr.ack_end_time = Some(now);

                let ack_rate = slr.ack_rate();
                let snd_rate = slr.snd_rate();
                if ack_rate >= snd_rate {
                    self.f_min_estimates.push_back(ack_rate);
                } else {
                    self.f_max_estimates.push_back(ack_rate);
                }

                let mut f_min = self
                    .f_min_estimates
                    .iter()
                    .copied()
                    .reduce(f64::max)
                    .unwrap();
                let mut f_max = self
                    .f_max_estimates
                    .iter()
                    .copied()
                    .reduce(f64::min)
                    .unwrap();

                // TODO: refactor state update and action
                if snd_rate <= ack_rate {
                    self.cwnd *= 1.25;

                    if self.ack_metric.is_some() {
                        let ndd_state = NDDState {
                            f_min,
                            f_max,
                            c_min: None,
                            c_max: None,
                            n_min: None,
                            n_max: None,
                            target_delay_min: None,
                            target_delay_max: None,
                            delay: delay.secs(),
                            min_rtt: self.min_rtt.secs(),
                        };
                        self.ack_metric.as_ref().unwrap().borrow_mut().log(ndd_state.to_row());
                    }
                }
                else {
                    if f_max < f_min {
                        // TODO: hack. In reality f can be fast changing. We
                        // should take more recent estimates of f.
                        // let tmp = f_max;
                        // f_max = f_min;
                        // f_min = tmp;
                        f_min = f_max;
                    }
                    let c_max = slr.c_estimate(f_min); // smaller f_estimate implies larger c_estimate
                    let c_min = slr.c_estimate(f_max);
                    assert!(c_max >= c_min);
                    let n1 = c_max / f_min;
                    let n2 = c_min / f_max;
                    assert!(n1 >= n2);
                    slr.n_min = Some(f64::max(1., f64::min(n1, n2)));
                    slr.n_max = Some(f64::max(n1, n2));

                    // update cwnd as we have a new complete record
                    let n_min = slr.n_min.unwrap();
                    let mut n_max = slr.n_max.unwrap();
                    if n_min == n_max {
                        n_max = n_min + 0.5;
                    }
                    let target_delay_min = n_min * self.min_rtt.secs();
                    let target_delay_max = n_max * self.min_rtt.secs();

                    if target_delay_min <= delay.secs() && delay.secs() <= target_delay_max {
                        if self.last_increase {
                            self.cwnd *= 0.75;
                        } else {
                            self.cwnd *= 1.25;
                        }
                    } else if delay.secs() < target_delay_min {
                        self.cwnd *= 1.25;
                    } else {
                        // delay.secs() > target_delay_max
                        self.cwnd *= 0.75;
                    }

                    if self.ack_metric.is_some() {
                        let ndd_state = NDDState {
                            f_min,
                            f_max,
                            c_min: Some(c_min),
                            c_max: Some(c_max),
                            n_min: Some(n_min),
                            n_max: Some(n_max),
                            target_delay_min: Some(target_delay_min),
                            target_delay_max: Some(target_delay_max),
                            delay: delay.secs(),
                            min_rtt: self.min_rtt.secs(),
                        };
                        self.ack_metric.as_ref().unwrap().borrow_mut().log(ndd_state.to_row());
                    }
                }

                self.cwnd = f64::max(MIN_CWND, self.cwnd);
                self.last_increase = self.prev_cwnd <= self.cwnd;
                self.prev_cwnd = self.cwnd;
            }
        }

        // TODO: should we average over the estimate of N?
        // only need to keep at most 3 records
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
            self.last_increase = self.prev_cwnd <= self.cwnd;
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
                n_min: None,
                n_max: None,
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