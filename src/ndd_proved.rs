use crate::metrics::{CsvMetric, MetricRegistry};
use crate::rtt_window::RTTWindow;
use crate::simulator::{PktId, SeqNum, Time};
use crate::transport::CongestionControl;

pub struct NDDProved {
    // PARAMETERS
    p_jitter_tolerance: Time,      // D
    p_cruise_quanta: Time,
    p_cruise_quanta_count: u32,    // T in units of quanta.
    p_cwnd_averaging_factor: f32,  // alpha
    p_cwnd_clamp_high: f64,        // delta1
    p_cwnd_clamp_low: f64,         // delta2
    p_probe_multiplier: f64,       // gamma1
    p_gamma2: f64,                 // gamma1 * (T+D)/T
    p_gamma3: f64,                 // gamma1 * (T-D)/T

    // STATE
    s_min_rtt: Time,

    // Collision slot state
    s_slot_start_time: Time,

    s_cruise_rate: f64,
    s_communicated_flow_count: f64,

    // Probe state
    s_probe_ongoing: bool,
    s_first_seq_of_probe: Option<SeqNum>,
    s_last_seq_of_probe: Option<SeqNum>,
    s_excess_delay: Time,
    s_excess_amount: f64,  // TODO: bytes?
}

impl CongestionControl for NDDProved {
    fn on_ack(&mut self, now: Time, cum_ack: SeqNum, ack_uid: PktId, rtt: Time, num_lost: u64) {
        // TODO: timeout min_rtt estimate
        self.min_rtt = std::cmp::min(self.min_rtt, rtt);

        // TODO: split into measurement updates and cwnd action?
        if self.probe_ongoing {
            update_excess_delay_if_allowed();
            if should_initiate_probe_end() {
                initiate_probe_end();
            }
            else if should_end_probe() {
                end_probe();
                update_cwnd();
            }
        } else {
            // Since I am not probing, I can estimate cruise rate, otherwise
            // cruise will overestimate.
            if cruise_quanta_elapsed() {
                add_cruise_entry();
                update_cruise_rate();
            }
            update_communicated_delay();
            if slot_ended() {
                // TODO: perhaps don't start probe and end round on the same
                // slot!
                if should_start_probe() {
                    start_probe();
                }
                if round_ended() {
                    reset_round_state()
                }
            }
        }
    }
}

impl NDDProved {
    fn should_initiate_probe_end() -> bool {
        false
    }

    fn should_end_probe() -> bool {
        false
    }

    fn is_ack_part_of_excess_duration(&self, ack: SeqNum) -> bool {
        if self.first_seq_of_probe.is_some() {
            if ack >= self.first_seq_of_probe.unwrap() {
                if self.last_seq_of_probe.is_some() {
                    ack <= self.last_seq_of_probe.unwrap()
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

    fn update_excess_delay_if_allowed(&mut self) {
        if self.is_ack_part_of_excess_duration(ack) {
            // update excess delay
            self.s_excess_delay = std::cmp::
        }
    }

    fn slot_ended() -> bool {
        // Slot duration is max {max rtprop + queueing delay, T}

        // TODO: Need to decide which queueing delay measurement to consider
        // here, so that all flows have roughly similar slot sizes.
    }

    fn update_cwnd(&mut self) {
        let bandwidth_estimate = self.s_excess_amount / self.s_excess_delay;
        let flow_count_estimate = bandwidth_estimate / self.s_cruise_estimate;
        let target_cwnd = self.cwnd * flow_count_estimate / self.s_communicated_flow_count;

        let prev_cwnd = cwnd;
        let mut next_cwnd = (1-self.p_cwnd_averaging_factor) * cwnd + self.p_cwnd_averaging_factor * target_cwnd;
        next_cwnd = max(next_cwnd, self.p_cwnd_clamp_low * prev_cwnd);
        next_cwnd = min(next_cwnd, self.p_cwnd_clamp_high * prev_cwnd);
        self.cwnd = next_cwnd;
        // TODO: should we round cwnd to bytes? Check what unit is cwnd
        // maintained in.
    }

    fn reset_round(self) {
        // reset cruise rate and communicated flow count estimates.
    }

    fn start_probe(self) {
        self.reset_probe_state()
    }

    fn reset_probe_state(self) {
        // start and end seq
        // excess amount and delay...
    }
}
