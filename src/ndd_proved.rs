use crate::metrics::{CsvMetric, MetricRegistry};
use crate::rtt_window::RTTWindow;
use crate::simulator::{PktId, SeqNum, Time};
use crate::transport::CongestionControl;

pub struct NDDProved {
    min_rtt: Time,
    probe_ongoing: bool,

    cruise_quanta: Time,
    cruise_quanta_count: u32,

    first_seq_of_probe: Option<SeqNum>,
    last_seq_of_probe: Option<SeqNum>,
}

impl CongestionControl for NDDProved {
    fn on_ack(&mut self, now: Time, cum_ack: SeqNum, ack_uid: PktId, rtt: Time, num_lost: u64) {
        // TODO: timeout min_rtt estimate
        self.min_rtt = std::cmp::min(self.min_rtt, rtt);

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
            if slot_ended() {
                if should_start_probe() {
                    start_probe();
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
        }
    }
}
