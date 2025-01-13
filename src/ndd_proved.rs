use crate::metrics::{CsvMetric, MetricRegistry};
use crate::rtt_window::RTTWindow;
use crate::simulator::{PktId, SeqNum, Time};
use crate::transport::CongestionControl;

/*
Slot duration choices:
1. Both measurement and actuation completes within slot. This requires a minimum
duration of E/C + probe_duration + rtprop, so we can either assume all rtprops
equal or use: E/C + probe_duration + max rtprop. Here queueing delay would be
an upper bound on E/C. Each flow can check max queueing delay since slot start
to see if slot has ended. Current implementation is based on this.

2. Measurement in a different slot. In this case, I may not be able to measure
within the same round if rtprop very large compared to round duration.
*/

pub struct NDDProved {
    // PARAMETERS
    p_jitter_tolerance: Time, // D in seconds
    p_cruise_quanta: Time,
    p_cruise_quanta_count: u32,   // T in units of quanta.
    p_cwnd_averaging_factor: f64, // alpha
    p_cwnd_clamp_high: f64,       // delta1
    p_cwnd_clamp_low: f64,        // delta2
    p_probe_multiplier: f64,      // gamma1
    p_gamma2: f64,                // gamma1 * (T+D)/T
    p_gamma3: f64,                // gamma1 * (T-D)/T
    p_probe_duration: Time,       // This can really be anything

    // STATE
    s_min_rtt: Time,
    s_cwnd: f64, // packets

    // CCAC style monotonically increasing packet counters.
    s_tot_tx: u64,
    // s_tot_rx: u64,
    // s_tot_ld: u64,

    // Collision slot state
    s_slot_start_time: Time,
    s_excess_delay_since_slot: Time,

    s_cruise_rate: f64, // packets per second
    s_communicated_flow_count: f64,

    // Probe state
    s_probe_ongoing: bool,
    s_probe_start_time: Time,
    s_cwnd_before_probe: f64,
    // TODO: Ideally we want the SeqNum to be monotonically increasing. In the
    // rust simulator, PktId need not be continuous (i.e., other flows may
    // request some of the ids), and the SeqNum need not be monotonic.
    s_first_seq_of_probe: Option<SeqNum>,
    s_last_seq_of_probe: Option<SeqNum>,
    s_probe_excess_delay: Time,
    s_probe_excess_amount: u64, // packets
}

impl CongestionControl for NDDProved {
    fn on_ack(&mut self, now: Time, cum_ack: SeqNum, ack_uid: PktId, rtt: Time, num_lost: u64) {
        // TODO: timeout min_rtt estimate
        self.s_min_rtt = std::cmp::min(self.s_min_rtt, rtt);

        // TODO: split into measurement updates and cwnd action?
        if self.s_probe_ongoing {
            self.update_excess_delay_if_allowed(cum_ack, rtt);
            if self.should_initiate_probe_end(now, rtt) {
                self.initiate_probe_end();
            } else if self.should_end_probe(cum_ack) {
                self.end_probe();
                self.update_cwnd();
            }
        } else {
            // Since I am not probing, I can estimate cruise rate, otherwise
            // cruise will overestimate.
            if self.cruise_quanta_elapsed() {
                self.add_cruise_entry();
                self.update_cruise_rate();
            }
            self.update_communicated_delay();
            if self.slot_ended() {
                // TODO: perhaps don't start probe and end round on the same
                // slot!
                if self.should_start_probe() {
                    self.start_probe();
                }
                if self.round_ended() {
                    self.reset_round_state()
                }
            }
        }
    }

    fn on_send(&mut self, _now: Time, _seq_num: SeqNum, _uid: PktId) {
        self.s_tot_tx += 1;
    }
}

impl NDDProved {
    fn should_initiate_probe_end(&self, now: Time, rtt: Time) -> bool {
        now - self.s_probe_start_time >= rtt // self.p_probe_duration
                                             // TODO: if last seq of probe has been sent!!
    }

    fn should_end_probe(&self, ack: SeqNum) -> bool {
        ack >= self.s_last_seq_of_probe.unwrap() + 1
    }

    fn is_ack_part_of_excess_duration(&self, ack: SeqNum) -> bool {
        assert!(self.s_probe_ongoing);
        if self.s_first_seq_of_probe.is_some() {
            if ack >= self.s_first_seq_of_probe.unwrap() {
                if self.s_last_seq_of_probe.is_some() {
                    ack <= self.s_last_seq_of_probe.unwrap()
                } else {
                    true
                }
            } else {
                false
            }
        } else {
            false
        }
    }

    fn update_excess_delay_if_allowed(&mut self, ack: SeqNum, rtt: Time) {
        if self.is_ack_part_of_excess_duration(ack) {
            // update excess delay
            let delay = rtt - self.s_min_rtt;
            self.s_probe_excess_delay = std::cmp::max(self.s_probe_excess_delay, delay);
        }
    }

    fn initiate_probe_end(&mut self) {
        self.s_cwnd = self.s_cwnd_before_probe;
    }

    fn end_probe(&mut self) {
        self.reset_probe_state();
        self.s_probe_ongoing = false;
    }

    fn slot_ended() -> bool {
        // Slot duration is max {max rtprop + queueing delay, T}

        // TODO: Need to decide which queueing delay measurement to consider
        // here, so that all flows have roughly similar slot sizes.
    }

    fn update_cwnd(&mut self) {
        let bandwidth_estimate = (self.s_probe_excess_amount as f64) / self.s_probe_excess_delay.secs(); // packets per second
        let flow_count_estimate = bandwidth_estimate / self.s_cruise_rate;
        let target_cwnd = self.s_cwnd * flow_count_estimate / self.s_communicated_flow_count;

        let prev_cwnd = self.s_cwnd;
        let mut next_cwnd = (1. - self.p_cwnd_averaging_factor) * prev_cwnd
            + self.p_cwnd_averaging_factor * target_cwnd;
        if next_cwnd > self.p_cwnd_clamp_high * prev_cwnd {
            next_cwnd = self.p_cwnd_clamp_high * prev_cwnd;
        }
        if next_cwnd < self.p_cwnd_clamp_low * prev_cwnd {
            next_cwnd = self.p_cwnd_clamp_low * prev_cwnd;
        }
        self.s_cwnd = next_cwnd;
    }

    fn reset_round(self) {
        // reset cruise rate and communicated flow count estimates.
    }

    fn start_probe(&mut self, now: Time) {
        self.reset_probe_state();
        self.s_probe_ongoing = true;
        self.s_probe_start_time = now; // TODO: should this be now or the time we have transmitted the first seq of probe?
        self.s_cwnd_before_probe = self.s_cwnd;
        self.s_probe_excess_delay = Time::from_micros(0);
        let s_excess_amount = f64::ceil(
            self.p_probe_multiplier
                * self.s_cruise_rate
                * self.s_communicated_flow_count
                * self.p_jitter_tolerance.secs(),
        );
        assert!(s_excess_amount >= 0.);
        self.s_probe_excess_amount = s_excess_amount as u64;
        self.s_first_seq_of_probe = Some(self.s_tot_tx + self.s_probe_excess_amount); // TODO: should we add 1 to this. I don't think so.
        self.s_last_seq_of_probe = None;
        self.s_cwnd = self.s_cwnd_before_probe + (self.s_probe_excess_amount as f64);
    }

    fn reset_probe_state(&mut self) {
        self.s_probe_ongoing = false;
        self.s_probe_start_time = Time::from_micros(0);
        self.s_cwnd_before_probe = 0.;
        self.s_first_seq_of_probe = None;
        self.s_last_seq_of_probe = None;
        self.s_probe_excess_delay = Time::from_micros(0);
        self.s_probe_excess_amount = 0;
    }
}
