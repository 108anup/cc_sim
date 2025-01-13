use crate::metrics::{CsvMetric, MetricRegistry};
use crate::ndd::MIN_CWND;
use crate::random;
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

// Ideally use this constant outside the struct so that all flows share the
// same value. Other parameters can be different for different flows and the
// system still works.

struct CruiseRecord {
    start_time: Time,
    start_tot_tx: u64,
    start_tot_rx: u64,
    start_tot_ld: u64,
    start_seq: SeqNum,

    end_time: Option<Time>,
    end_tot_tx: Option<u64>,
    end_tot_rx: Option<u64>,
    end_tot_ld: Option<u64>,
    end_seq: Option<SeqNum>,
}

impl CruiseRecord {
    fn new(
        start_time: Time,
        start_tot_tx: u64,
        start_tot_rx: u64,
        start_tot_ld: u64,
        start_seq: SeqNum,
    ) -> Self {
        CruiseRecord {
            start_time,
            start_tot_tx,
            start_tot_rx,
            start_tot_ld,
            start_seq,
            end_time: None,
            end_tot_tx: None,
            end_tot_rx: None,
            end_tot_ld: None,
            end_seq: None,
        }
    }

    fn fill_end(
        &mut self,
        end_time: Time,
        end_tot_tx: u64,
        end_tot_rx: u64,
        end_tot_ld: u64,
        end_seq: SeqNum,
    ) {
        self.end_time = Some(end_time);
        self.end_tot_tx = Some(end_tot_tx);
        self.end_tot_rx = Some(end_tot_rx);
        self.end_tot_ld = Some(end_tot_ld);
        self.end_seq = Some(end_seq);
    }

    fn get_ack_rate(&self) -> f64 {
        // packets per second
        let duration = self.end_time.unwrap() - self.start_time;
        let packets = self.end_tot_rx.unwrap() - self.start_tot_rx;
        packets as f64 / duration.secs()
    }
}

struct DelayRecord {
    time: Time,
    queueing_delay: Time,
}

pub struct NDDProved {
    // PARAMETERS
    p_jitter_tolerance: Time, // D in seconds
    p_cruise_quanta: Time,
    p_cruise_quanta_count: u64,   // T in units of quanta.
    p_cwnd_averaging_factor: f64, // alpha
    p_cwnd_clamp_high: f64,       // delta1
    p_cwnd_clamp_low: f64,        // delta2
    p_probe_multiplier: f64,      // gamma1
    p_gamma2: f64,                // gamma1 * (T+D)/T
    p_gamma3: f64,                // gamma1 * (T-D)/T
    p_probe_duration: Time,       // This can really be anything
    p_min_cwnd: f64,              // packets
    p_contract_min_delay: Time,
    p_max_rtprop: Time,
    p_max_flow_count: u64,
    p_probe_probability: f64,     // = 1/p_max_flow_count, so that in expectation we have one probe per round.

    // STATE
    s_min_rtt: Time,
    s_cwnd: f64, // packets

    // CCAC style monotonically increasing packet counters.
    s_tot_tx: u64,
    s_tot_rx: u64,
    s_tot_ld: u64,
    // TODO: I don't think CCAC formulation allowed false positives in loss
    // detection. So this might not work correctly.

    // Collision slot state (Slot level)
    s_slot_start_time: Time,
    s_slot_queueing_delay: Time,  // Upper bound on E/C

    // State for N_R estimate (Round level)
    s_slots_till_now_in_this_round: u64,
    s_communicated_flow_count_this_round: f64,
    // s_queueing_delay_records: Vec<DelayRecord>,

    // Cruise state
    s_cruise_rate_this_round: f64, // packets per second
    s_cruise_records: Vec<CruiseRecord>,

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
        self.s_tot_rx += 1;
        self.s_tot_ld += num_lost;

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
            self.check_update_cruise_state_and_rate(now, cum_ack);
            // TODO: we want to update cruise state (cruise slots) even when
            // there are probes. Otherwise, cruise rate will be underestimated
            // (as there will be time gaps). We want to mark such quantas as
            // probing so that we do not use them for cruise rate estimation.
            self.update_communicated_flow_count(now, rtt);
            self.update_slot_state(now, rtt);
            if self.slot_ended(now) {
                // TODO: double check ordering of the three parts of this
                // scope. Its not the most intuitive to read/flow. should
                // start_new_slot before or after round end?
                self.start_new_slot(now, rtt);


                // TODO: what is the best way to order round end, probe and
                // cruise slots. At the minimum we want one cruise slot before
                // probe and after round end, so that we can estimate cruise
                // rate for probe. Alternatively, on round end we could
                // preserve information about cruise from last cruise slot of
                // last round.

                // We can force to start probe only after we have made some
                // cruise observations. We can end round right after probe.
                // Probe could always be last slot of round.

                if self.round_ended() {
                    self.reset_round_state()
                }
                else if self.should_start_probe() {
                    // The else ensures that we don't start probe and end round
                    // at the same time. As a result, one cruise slot always
                    // happens before probe.
                    self.start_probe(now);
                }
            }
        }
    }

    fn on_send(&mut self, _now: Time, _seq_num: SeqNum, _uid: PktId) {
        self.s_tot_tx += 1;
    }

    fn get_cwnd(&mut self) -> u64 {
        self.s_cwnd as u64
        // TODO: should we ceil it, by default it would floor? Since we have a
        // MIN_CWND cap, I think it should be fine.
    }

    // TODO: fill boilerplate below
    fn get_intersend_time(&mut self) -> Time {
        std::cmp::max(
            MIN_INTERSEND_TIME,
            Time::from_micros((2e6 * self.base_rtt.get_srtt().secs() / self.s_cwnd) as u64),
        )
    }

    fn on_timeout(&mut self) {
        self.cwnd = MIN_CWND;
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
        // self.phase = rand::random::<u32>() % CYCLE_STEPS;
        self.phase = (2 * str::parse::<u32>(name).unwrap()) % CYCLE_STEPS;
    }

    fn finish(&self) {
        if let Some(metric_registry) = &self.metric_registry {
            metric_registry.finish();
        }
    }
}

impl NDDProved {
    fn should_initiate_probe_end(&self, now: Time, rtt: Time) -> bool {
        now - self.s_probe_start_time >= self.p_probe_duration
    }

    fn should_end_probe(&self, ack: SeqNum) -> bool {
        self.s_tot_rx + self.s_tot_ld >= self.s_last_seq_of_probe.unwrap() + 1
    }

    fn is_ack_part_of_excess_duration(&self, _ack: SeqNum) -> bool {
        let ack = self.s_tot_rx + self.s_tot_ld;
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

    fn update_excess_delay_if_allowed(&mut self, _ack: SeqNum, rtt: Time) {
        if self.is_ack_part_of_excess_duration(_ack) {
            // update excess delay
            let delay = rtt - self.s_min_rtt;
            self.s_probe_excess_delay = std::cmp::max(self.s_probe_excess_delay, delay);
        }
    }

    fn initiate_probe_end(&mut self) {
        self.s_last_seq_of_probe = Some(self.s_tot_tx);
        self.s_cwnd = self.s_cwnd_before_probe;
    }

    fn end_probe(&mut self) {
        self.s_probe_ongoing = false;
    }

    fn update_cwnd(&mut self) {
        let bandwidth_estimate =
            (self.s_probe_excess_amount as f64) / self.s_probe_excess_delay.secs(); // packets per second
        let flow_count_estimate = bandwidth_estimate / self.s_cruise_rate_this_round;
        let target_cwnd =
            self.s_cwnd * flow_count_estimate / self.s_communicated_flow_count_this_round;

        let prev_cwnd = self.s_cwnd;
        let mut next_cwnd = (1. - self.p_cwnd_averaging_factor) * prev_cwnd
            + self.p_cwnd_averaging_factor * target_cwnd;
        if next_cwnd > self.p_cwnd_clamp_high * prev_cwnd {
            next_cwnd = self.p_cwnd_clamp_high * prev_cwnd;
        }
        if next_cwnd < self.p_cwnd_clamp_low * prev_cwnd {
            next_cwnd = self.p_cwnd_clamp_low * prev_cwnd;
        }
        if next_cwnd < self.p_min_cwnd {
            next_cwnd = self.p_min_cwnd;
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
                * self.s_cruise_rate_this_round
                * self.s_communicated_flow_count_this_round
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

    fn check_update_cruise_state_and_rate(&mut self, now: Time, ack: SeqNum) {
        if self.cruise_quanta_elapsed(now) {
            self.fill_cruise_entry(now, ack);
            self.update_cruise_rate();
            self.add_cruise_entry(now, ack);
        }
    }

    fn cruise_quanta_elapsed(&self, now: Time) -> bool {
        now >= self.s_cruise_records.last().unwrap().start_time + self.p_cruise_quanta
    }

    fn fill_cruise_entry(&mut self, now: Time, ack: SeqNum) {
        self.s_cruise_records.last().unwrap().fill_end(
            now,
            self.s_tot_tx,
            self.s_tot_rx,
            self.s_tot_ld,
            ack,
        );
    }

    fn add_cruise_entry(&mut self, now: Time, ack: SeqNum) {
        self.s_cruise_records.push(CruiseRecord::new(
            now,
            self.s_tot_tx,
            self.s_tot_rx,
            self.s_tot_ld,
            ack,
        ));
    }

    fn update_cruise_rate(&mut self) {
        let last_cruise_rate = self.s_cruise_records.last().unwrap().get_ack_rate();
        if self.s_cruise_rate_this_round > last_cruise_rate {
            self.s_cruise_rate_this_round = last_cruise_rate;
        }
    }

    fn update_communicated_flow_count(&mut self, now: Time, rtt: Time) {
        let queueing_delay = rtt - self.s_min_rtt;
        // self.s_queueing_delay_records.push(DelayRecord {
        //     time: now,
        //     queueing_delay,
        // });

        // min delay over the round (not just slot)
        let this_flow_count =
            (queueing_delay.micros() as f64) / (self.p_contract_min_delay.micros() as f64);
        if self.s_communicated_flow_count_this_round > this_flow_count {
            self.s_communicated_flow_count_this_round = this_flow_count;
        }
        if self.s_communicated_flow_count_this_round < 1. {
            self.s_communicated_flow_count_this_round = 1.;
        }
        if self.s_communicated_flow_count_this_round > self.p_max_flow_count as f64 {
            self.s_communicated_flow_count_this_round = self.p_max_flow_count as f64;
        }
    }

    fn slot_ended(&self, now: Time) -> bool {
        // Slot duration is max {max rtprop + queueing delay + probe_duration, T}

        // TODO: We want queueing delay measurement, so that all flows have
        // roughly similar slot sizes. Currently taken min queueing delay of
        // latest slot.

        let mut slot_duration = self.p_max_rtprop + self.p_probe_duration + self.s_slot_queueing_delay;
        if slot_duration < self.p_cruise_quanta * self.p_cruise_quanta_count {
            slot_duration = self.p_cruise_quanta;
        }

        now >= self.s_slot_start_time + slot_duration
    }

    fn update_slot_state(&self, now: Time, rtt: Time) {
        // max delay over the slot
        let queueing_delay = rtt - self.s_min_rtt;
        if self.s_slot_queueing_delay < queueing_delay {
            self.s_slot_queueing_delay = queueing_delay;
        }
    }

    fn start_new_slot(&mut self, now: Time, rtt: Time) {
        self.s_slot_start_time = now;
        self.s_slot_queueing_delay = rtt - self.s_min_rtt;
        self.s_slots_till_now_in_this_round += 1;
    }

    fn round_ended(self) -> bool {
        self.s_slots_till_now_in_this_round >= self.p_max_flow_count
    }

    fn reset_round_state(&mut self) {
        // Since we do not probe on round end, and are guaranteed to have a
        // cruise after round end and before probe, we can clear all the round
        // state.

        self.s_slots_till_now_in_this_round = 0; // count
        self.s_communicated_flow_count_this_round = self.p_max_flow_count as f64;  // min
        // self.s_queueing_delay_records.clear();  // min
        let last_record = *self.s_cruise_records.last().unwrap();
        self.s_cruise_records.clear();
        self.s_cruise_records.push(last_record);
        self.s_cruise_rate_this_round = 0.;  // max
    }

    fn should_start_probe(&self) -> bool {
        // output true with probability self.p_probe_probability
    }
}
