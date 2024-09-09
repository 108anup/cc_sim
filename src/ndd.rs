// NDD: CCA that maintains O(N * D) delay when there are N flows.

use std::collections::VecDeque;

use circular_buffer::CircularBuffer;

use crate::rtt_window::RTTWindow;
use crate::simulator::{PktId, SeqNum, Time};
use crate::transport::CongestionControl;

pub const F_FILTER_LEN: usize = 10;

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

        (snd_rate * ack_rate - f_estimate * ack_rate) / (snd_rate - ack_rate)
    }
}

pub struct NDD {
    base_rtt: RTTWindow,
    f_min_estimates: CircularBuffer<F_FILTER_LEN, f64>,
    f_max_estimates: CircularBuffer<F_FILTER_LEN, f64>,
    records: VecDeque<Record>,
    cwnd: f64,
    prev_cwnd: f64,
    last_increase: bool,
}

impl Default for NDD {
    fn default() -> Self {
        Self {
            base_rtt: RTTWindow::new(Time::from_secs(10)),
            f_min_estimates: CircularBuffer::new(),
            f_max_estimates: CircularBuffer::new(),
            records: VecDeque::new(),
            cwnd: 2.,
            prev_cwnd: 2.,
            last_increase: false,
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

        let base_rtt = self.base_rtt.get_min_rtt().unwrap();
        let delay = rtt - base_rtt;

        // Record
        let last_record = self.records.back_mut().unwrap();
        // ^^ We can only get ack if we sent something so this should always be
        // Some
        if _cum_ack == last_record.snd_beg_seq {
            last_record.snd_complete = true;
            last_record.ack_beg_seq = Some(_cum_ack);
            last_record.ack_beg_time = Some(now);
        }
        else if _cum_ack < last_record.snd_beg_seq {
            // There must be a record before last one whose ACKs have not
            // completed
            let slr = self.records.iter_mut().rev().nth(1).unwrap(); // second last record
            if _cum_ack == slr.snd_end_seq.unwrap() {
                slr.ack_complete = true;
                slr.ack_end_seq = Some(_cum_ack);
                slr.ack_end_time = Some(now);
            }

            let ack_rate = slr.ack_rate();
            let snd_rate = slr.snd_rate();
            if ack_rate >= snd_rate {
                self.f_min_estimates.push_back(ack_rate);
            }
            else {
                self.f_max_estimates.push_back(ack_rate);
            }

            let f_min = self.f_min_estimates.iter().copied().reduce(f64::max).unwrap();
            let f_max = self.f_max_estimates.iter().copied().reduce(f64::min).unwrap();
            let c1 = slr.c_estimate(f_min);
            let c2 = slr.c_estimate(f_max);
            let n1 = c1/f_min;
            let n2 = c2/f_max;
            slr.n_min = Some(f64::max(1., f64::min(n1, n2)));
            slr.n_max = Some(f64::max(n1, n2));

            // update cwnd as we have a new complete record
            let n_min = slr.n_min.unwrap();
            let n_max = slr.n_max.unwrap();
            let target_delay_min = n_min * base_rtt.secs();
            let target_delay_max = n_max * base_rtt.secs();

            if target_delay_min <= delay.secs() && delay.secs() <= target_delay_max {
                if self.last_increase {
                    self.cwnd *= 0.75;
                }
                else {
                    self.cwnd *= 1.25;
                }
            }
            else if delay.secs() < target_delay_min {
                self.cwnd *= 1.25;
            }
            else {
                // delay.secs() > target_delay_max
                self.cwnd *= 0.75;
            }

            self.cwnd = f64::max(2., self.cwnd);
            self.last_increase = self.prev_cwnd <= self.cwnd;
            self.prev_cwnd = self.cwnd;
        }

        // TODO: should we average over the estimate of N?
        // only need to keep at most 3 records
        while self.records.len() > 3 {
            self.records.pop_front();
        }

        let last_complete_record = self.records.iter().rev().find(|r| r.ack_complete);
        if last_complete_record.is_none() {
            // Slow start like stuff when we don't have last record
            if delay.secs() <= 1.5 * base_rtt.secs() {
                self.cwnd += 1.;
            }
            else {
                self.cwnd -= 1./2.;
            }
            self.cwnd = f64::max(2., self.cwnd);
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
        Time::from_micros((2e6 * self.base_rtt.get_srtt().secs() / self.cwnd) as u64)
    }
}
