use std::cell::RefCell;
use std::fmt::Display;
use std::rc::Rc;

use num::Float;
use rand::prelude::*;
use rand::rngs::StdRng;
use rand_seeder::Seeder;
use serde::Serialize;

use crate::metrics::{CsvMetric, CsvMetricStruct, MetricRegistry};
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

fn float_max<T: Float>(current: T, sample: T) -> T {
    if sample > current {
        sample
    } else {
        current
    }
}

fn float_min<T: Float>(current: T, sample: T) -> T {
    if sample < current {
        sample
    } else {
        current
    }
}

#[derive(Serialize, Default)]
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

    duration: Option<Time>,
    acked: Option<u64>,
    ack_rate: Option<f64>,

    probe_ongoing: bool,
}

impl CruiseRecord {
    fn new(
        start_time: Time,
        start_tot_tx: u64,
        start_tot_rx: u64,
        start_tot_ld: u64,
        start_seq: SeqNum,
        probe_ongoing: bool,
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
            duration: None,
            acked: None,
            ack_rate: None,
            probe_ongoing,
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
        assert!(end_time > self.start_time);
        self.end_time = Some(end_time);
        self.end_tot_tx = Some(end_tot_tx);
        self.end_tot_rx = Some(end_tot_rx);
        self.end_tot_ld = Some(end_tot_ld);
        self.end_seq = Some(end_seq);
        self.duration = Some(end_time - self.start_time);
        self.acked = Some(end_tot_rx - self.start_tot_rx);
        self.ack_rate = Some(self.acked.unwrap() as f64 / self.duration.unwrap().secs());
    }
}

impl CsvMetricStruct for CruiseRecord {}

#[derive(Serialize, Default)]
struct AckRecord {
    time: Time,
    tot_tx: u64,
    tot_rx: u64,
    tot_ld: u64,
    cum_ack: SeqNum,
    rtt: Time,
    probe_ongoing: bool,
    is_part_of_excess_duration: bool,
    cwnd: f64,
    inflight: u64,
}

impl CsvMetricStruct for AckRecord {}


#[derive(Serialize, Default)]
struct SendRecord {
    time: Time,
    tot_tx: u64,
    tot_rx: u64,
    tot_ld: u64,
    probe_ongoing: bool,
    cwnd: f64,
    inflight: u64,
}

impl CsvMetricStruct for SendRecord {}

struct QdelRecord {
    time: Time,
    qdel: Time,
}

pub struct NDDProved {
    name: String,
    rng: StdRng,
    p_rng_seed: u64,

    // -------------------------------------------------------------------------
    // METRICS
    metric_registry: Option<MetricRegistry>,
    slot_metric: Option<Rc<RefCell<CsvMetric>>>,
    cwnd_update_metric: Option<Rc<RefCell<CsvMetric>>>,
    cruise_metric: Option<Rc<RefCell<CsvMetric>>>,
    ack_metric: Option<Rc<RefCell<CsvMetric>>>,
    send_metric: Option<Rc<RefCell<CsvMetric>>>,

    // -------------------------------------------------------------------------
    // PARAMETERS
    // Design parameters (derived from objectives)
    p_cruise_measurement_duration: Time,
    p_cruise_measurement_duration_multiplier: u64, // T in units of jitter.
    p_cwnd_averaging_factor: f64,                  // alpha
    p_cwnd_clamp_high: f64,                        // delta1
    p_cwnd_clamp_low: f64,                         // delta2
    p_probe_multiplier: f64,                       // gamma1
    // p_gamma2: f64,                              // gamma1 * (T+D)/T
    // p_gamma3: f64,                              // gamma1 * (T-D)/T
    p_probe_duration: Time, // This can really be anything
    p_contract_min_delay: Time,
    p_slot_load_factor: u64,
    p_probe_probability: f64, // = 1/(p_slot_load_factor * p_max_flow_count), so that in expectation we have one probe per round.

    // Prior belief of network parameters (upper (ub) and lower (lb) bounds)
    p_ub_flow_count: u64,
    p_ub_jitter: Time, // D
    p_ub_rtprop: Time,
    p_lb_cwnd: f64, // packets
    p_lb_intersend_time: Time,

    // -------------------------------------------------------------------------
    // STATE
    s_latest_rtt: Time,
    s_srtt: RTTWindow,
    s_min_rtt: Time,
    s_cwnd: f64, // packets

    // CCAC style monotonically increasing packet counters.
    s_tot_tx: u64,
    s_tot_rx: u64,
    s_tot_ld: u64,
    // ? I don't think CCAC formulation allowed false positives in loss
    // detection. So this might not work correctly.

    // Collision slot state (Slot level)
    s_slot_start_time: Time,
    s_slot_max_qdel: Time,         // Upper bound on E/C
    s_slot_min_qdel: Option<Time>, // For computing excess delay when probing.

    // State for N_R estimate (Round level)
    s_round_slots_till_now: u64,
    s_round_communicated_flow_count: f64,
    // s_qdel_records: Vec<QdelRecord>,

    // Cruise state (get record/sample every T time (independent of slot time)
    s_round_max_cruise_rate: f64, // packets per second
    s_round_cruise_records: Vec<CruiseRecord>,
    s_latest_cruise_rate: f64, // packets per second

    // Probe state (for the slot in which we probe, the probe may be smaller
    // than the slot duration)
    s_probe_ongoing: bool,
    s_probe_initiated_end: bool,
    s_probe_start_time: Option<Time>,
    s_probe_cwnd_before: f64,
    s_probe_min_qdel_before: Time,
    s_probe_first_seq: Option<u64>,
    s_probe_last_seq: Option<u64>,
    s_probe_drain_last_seq: Option<u64>,
    s_probe_min_qdel_during: Option<Time>,
    s_probe_excess_amount: u64, // packets
}

impl Display for NDDProved {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // output all the parameters
        writeln!(f, "NDDProved parameters:")?;
        writeln!(f, "p_rng_seed: {}", self.p_rng_seed)?;
        writeln!(
            f,
            "p_cruise_measurement_duration: {}",
            self.p_cruise_measurement_duration
        )?;
        writeln!(
            f,
            "p_cruise_measurement_duration_multiplier: {}",
            self.p_cruise_measurement_duration_multiplier
        )?;
        writeln!(
            f,
            "p_cwnd_averaging_factor: {}",
            self.p_cwnd_averaging_factor
        )?;
        writeln!(f, "p_cwnd_clamp_high: {}", self.p_cwnd_clamp_high)?;
        writeln!(f, "p_cwnd_clamp_low: {}", self.p_cwnd_clamp_low)?;
        writeln!(f, "p_probe_multiplier: {}", self.p_probe_multiplier)?;
        writeln!(f, "p_probe_duration: {}", self.p_probe_duration)?;
        writeln!(f, "p_contract_min_delay: {}", self.p_contract_min_delay)?;
        writeln!(f, "p_slot_load_factor: {}", self.p_slot_load_factor)?;
        writeln!(f, "p_probe_probability: {}", self.p_probe_probability)?;
        writeln!(f, "p_ub_flow_count: {}", self.p_ub_flow_count)?;
        writeln!(f, "p_ub_jitter: {}", self.p_ub_jitter)?;
        writeln!(f, "p_ub_rtprop: {}", self.p_ub_rtprop)?;
        writeln!(f, "p_lb_cwnd: {}", self.p_lb_cwnd)?;
        writeln!(f, "p_lb_intersend_time: {}", self.p_lb_intersend_time)?;
        Ok(())
    }
}

// TODO: write derive macro to convert struct to csv row.
#[derive(Serialize, Default)]
struct CwndUpdateMetric {
    now: Time,
    probe_cwnd_before: f64,
    probe_min_qdel_before: Time,
    probe_min_qdel_during: Time,
    probe_excess_amount: u64,
    probe_excess_qdel: Time,

    communicated_flow_count: f64,
    round_max_cruise_rate: f64,
    bandwidth_estimate: Option<f64>,
    latest_cruise_rate: f64,
    flow_count_estimate: Option<f64>,
    target_cwnd: Option<f64>,

    probe_cwnd_after: f64,
}

impl CsvMetricStruct for CwndUpdateMetric {}

#[derive(Serialize, Default)]
struct SlotMetric {
    start_time: Time,
    end_time: Time,
    duration: Time,
    min_qdel: Time,
    max_qdel: Time,
    communicated_flow_count: f64,
    round_max_cruise_rate: f64,
    latest_cruise_rate: f64,
    cruise_happened: bool,
    probe_happened: bool,
    round_ended: bool,
    cwnd: f64,
}

impl CsvMetricStruct for SlotMetric {}

impl CongestionControl for NDDProved {
    fn on_ack(&mut self, now: Time, cum_ack: SeqNum, ack_uid: PktId, rtt: Time, num_lost: u64) {
        self.s_tot_rx += 1;
        self.s_tot_ld += num_lost;
        self.s_latest_rtt = rtt;

        self.s_srtt.new_rtt_sample(rtt, now);
        self.s_min_rtt = std::cmp::min(self.s_min_rtt, rtt);
        // TODO: timeout min_rtt estimate

        // ? split into measurement updates and cwnd action?
        self.check_update_cruise_state_and_rate(now, cum_ack);

        // ? We from from old state to next state. Since it is multi-variable,
        // we update the state variable by variable, but sometimes we need
        // access to old state variable. Currently, we have carefully chosen
        // the order to avoid this. Ideally, we can keep old and next state and
        // swap them at the end.

        let mut probe_ended = false;
        let mut cruise_ended = false;
        if self.s_probe_ongoing {
            self.update_probe_delay_if_allowed(cum_ack, rtt);
            if !self.s_probe_initiated_end && self.should_initiate_probe_end(now, rtt) {
                self.initiate_probe_end();
            }
            if self.s_probe_initiated_end && self.should_end_probe(cum_ack) {
                self.end_probe();
                self.update_cwnd_after_probe(now);
                probe_ended = true;
            }
        } else {
            // cruise ongoing
            self.update_communicated_flow_count(now, rtt);
            self.update_slot_state(now, rtt);
            cruise_ended = self.slot_ended(now);
        }

        if cruise_ended || probe_ended {
            // slot ended

            // TODO: we can follow convention that round ends after probe so
            // that we maximize information gathering?
            let round_ended = self.round_ended();
            self.log_slot_metric(now, cruise_ended, probe_ended, round_ended);

            if round_ended {
                self.reset_round_state()
            }

            if self.s_round_slots_till_now > 1 {
                // NOTE: the slots > 1 allows us to have at least one
                // cruise slot before any probe. Since round of flows need
                // not overlap, this does not necessarily affect collision
                // probability.
                if self.should_start_probe() {
                    self.start_probe(now);
                }
            }

            self.start_new_slot(now, rtt);
        }

        self.log_ack_metric(now, cum_ack, rtt);
    }

    fn on_send(&mut self, now: Time, _seq_num: SeqNum, _uid: PktId) {
        self.s_tot_tx += 1;

        if self.s_probe_ongoing && self.s_probe_start_time.is_none() && self.s_tot_tx >= self.s_probe_first_seq.unwrap() {
            self.s_probe_start_time = Some(now);
        }

        self.log_send_metric(now);
    }

    fn get_cwnd(&mut self) -> u64 {
        self.s_cwnd as u64
        // ? should we ceil it, by default it would floor? Since we have a
        // p_min_cwnd lower cap, I think it should be fine.
    }

    fn get_intersend_time(&mut self) -> Time {
        // TODO: is this the best rate value for cwnd?

        // If we pace during probe, then do we do not get correct bandwidth
        // estimate. If we do not pace during cruise, then we create
        // self-induced jitter.

        // if self.s_probe_ongoing {
        //     self.p_lb_intersend_time
        // }
        // else {
        //     std::cmp::max(
        //         self.p_lb_intersend_time,
        //         Time::from_micros((2e6 * self.s_srtt.get_srtt().secs() / self.s_cwnd) as u64),
        //     )
        // }

        // All the following work. Pick any. With jitter, there may be burst of
        // ACKs, so we should pace new transmissions.

        // TODO: Ideally we can use min RTT in this slot.

        // std::cmp::max(
        //     self.p_lb_intersend_time,
        //     Time::from_micros((self.s_latest_rtt.micros() as f64 / (self.s_cwnd * 2.)) as u64),
        // );

        std::cmp::max(
            self.p_lb_intersend_time,
            Time::from_micros((self.s_srtt.get_srtt().micros() as f64 / (self.s_cwnd * 2.)) as u64),
        )

        // self.p_lb_intersend_time
    }

    fn on_timeout(&mut self) {
        self.s_cwnd = self.p_lb_cwnd;
    }

    fn init(&mut self, name: &str, metrics_config_file: Option<String>) {
        self.name = name.to_string();
        self.init_metrics(metrics_config_file);
        self.reset_round_state();
        self.reset_probe_state();
        // self.rng = StdRng::seed_from_u64(self.p_rng_seed);
        let unique_str = &(name.to_owned() + self.p_rng_seed.to_string().as_str());
        let seed = Seeder::from(unique_str).make_seed::<[u8; 32]>();
        self.rng = StdRng::from_seed(seed);

        println!("Initialized NDDProved {}", name);
        println!("{}", self);
    }

    fn finish(&self) {
        if let Some(metric_registry) = &self.metric_registry {
            metric_registry.finish();
        }
    }
}

impl NDDProved {

    fn init_metrics(&mut self, metrics_config_file_: Option<String>) {
        if let Some(metrics_config_file) = metrics_config_file_ {
            self.metric_registry = Some(MetricRegistry::new(&metrics_config_file));
        }
        self.slot_metric = self
            .metric_registry
            .as_mut()
            .unwrap()
            .register_csv_metric(&(self.name.to_owned() + "slot"), SlotMetric::get_columns());
        self.cwnd_update_metric = self.metric_registry.as_mut().unwrap().register_csv_metric(
            &(self.name.to_owned() + "cwnd_update"),
            CwndUpdateMetric::get_columns(),
        );
        self.cruise_metric = self.metric_registry.as_mut().unwrap().register_csv_metric(
            &(self.name.to_owned() + "cruise"),
            CruiseRecord::get_columns(),
        );
        self.ack_metric = self.metric_registry.as_mut().unwrap().register_csv_metric(
            &(self.name.to_owned() + "ack"),
            AckRecord::get_columns(),
        );
        self.send_metric = self.metric_registry.as_mut().unwrap().register_csv_metric(
            &(self.name.to_owned() + "send"),
            SendRecord::get_columns(),
        );
    }

    fn log_send_metric(&self, now: Time) {
        self.send_metric.as_ref().unwrap().borrow_mut().log(
            SendRecord {
                time: now,
                tot_tx: self.s_tot_tx,
                tot_rx: self.s_tot_rx,
                tot_ld: self.s_tot_ld,
                probe_ongoing: self.s_probe_ongoing,
                cwnd: self.s_cwnd,
                inflight: self.s_tot_tx - self.s_tot_rx - self.s_tot_ld,
            }.to_row()
        )
    }

    fn log_ack_metric(&self, now: Time, cum_ack: SeqNum, rtt: Time) {
        self.ack_metric.as_ref().unwrap().borrow_mut().log(
            AckRecord {
                time: now,
                tot_tx: self.s_tot_tx,
                tot_rx: self.s_tot_rx,
                tot_ld: self.s_tot_ld,
                cum_ack,
                rtt,
                probe_ongoing: self.s_probe_ongoing,
                is_part_of_excess_duration: self.s_probe_ongoing && self.is_ack_part_of_excess_duration(cum_ack),
                cwnd: self.s_cwnd,
                inflight: self.s_tot_tx - self.s_tot_rx - self.s_tot_ld,
            }.to_row()
        );
    }

    fn should_initiate_probe_end(&self, now: Time, rtt: Time) -> bool {
        if self.s_probe_start_time.is_none() {
            false
        }
        else {
            now - self.s_probe_start_time.unwrap() >= self.p_probe_duration
        }
    }

    fn should_end_probe(&self, ack: SeqNum) -> bool {
        // TODO: is it really true that this is the last packet with any excess
        // delay. After cwnd drop, delay should decrease linearly right?
        // self.s_tot_rx + self.s_tot_ld >= self.s_probe_last_seq.unwrap() + 1
        self.s_tot_rx + self.s_tot_ld >= self.s_probe_drain_last_seq.unwrap() + 1
    }

    fn is_ack_part_of_excess_duration(&self, _ack: SeqNum) -> bool {
        let ack = self.s_tot_rx + self.s_tot_ld;
        assert!(self.s_probe_ongoing);
        if self.s_probe_first_seq.is_some() {
            if ack >= self.s_probe_first_seq.unwrap() {
                if self.s_probe_last_seq.is_some() {
                    ack <= self.s_probe_last_seq.unwrap()
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

    fn update_probe_delay_if_allowed(&mut self, _ack: SeqNum, rtt: Time) {
        if self.is_ack_part_of_excess_duration(_ack) {
            // update excess delay
            let delay = rtt - self.s_min_rtt;
            if self.s_probe_min_qdel_during.is_none() {
                self.s_probe_min_qdel_during = Some(delay);
            }
            self.s_probe_min_qdel_during =
                Some(std::cmp::min(self.s_probe_min_qdel_during.unwrap(), delay));
        }
    }

    fn initiate_probe_end(&mut self) {
        self.s_probe_last_seq = Some(std::cmp::max(
            self.s_probe_first_seq.unwrap(),
            self.s_tot_tx,
        ));
        // The max ensures that even if probe duration is very small (if probe
        // rate too low), the probe contains at least 1 pkt which experiences
        // full delay.
        self.s_cwnd = self.s_probe_cwnd_before;
        self.s_probe_initiated_end = true;
        self.s_probe_drain_last_seq = Some(self.s_probe_last_seq.unwrap() + (f64::ceil(self.s_cwnd) as u64));
    }

    fn end_probe(&mut self) {
        self.s_probe_ongoing = false;
    }

    fn log_cwnd_update(
        &self,
        now: Time,
        probe_cwnd_before: f64,
        probe_excess_qdel: Time,
        bandwidth_estimate: Option<f64>,
        flow_count_estimate: Option<f64>,
        target_cwnd: Option<f64>,
        probe_cwnd_after: f64,
    ) {
        self.cwnd_update_metric.as_ref().unwrap().borrow_mut().log(
            CwndUpdateMetric {
                now,
                probe_cwnd_before,
                probe_min_qdel_before: self.s_probe_min_qdel_before,
                probe_min_qdel_during: self.s_probe_min_qdel_during.unwrap(),
                probe_excess_amount: self.s_probe_excess_amount,
                probe_excess_qdel,
                bandwidth_estimate,
                round_max_cruise_rate: self.s_round_max_cruise_rate,
                latest_cruise_rate: self.s_latest_cruise_rate,
                flow_count_estimate,
                communicated_flow_count: self.s_round_communicated_flow_count,
                target_cwnd,
                probe_cwnd_after,
            }
            .to_row(),
        );
    }

    fn update_cwnd_after_probe(&mut self, now: Time) {
        let prev_cwnd = self.s_cwnd;
        let mut next_cwnd = self.p_cwnd_clamp_high * prev_cwnd;
        let mut log_bandwidth_estimate = None;
        let mut log_flow_count_estimate = None;
        let mut log_target_cwnd = None;
        let mut probe_excess_qdel = Time::from_millis(0);

        if self.s_probe_min_qdel_before < self.s_probe_min_qdel_during.unwrap() && self.s_round_communicated_flow_count > 0. {
            probe_excess_qdel =
                self.s_probe_min_qdel_during.unwrap() - self.s_probe_min_qdel_before;
            let bandwidth_estimate = (self.s_probe_excess_amount as f64) / probe_excess_qdel.secs(); // packets per second
            let mut flow_count_estimate = bandwidth_estimate / self.s_round_max_cruise_rate;
            flow_count_estimate = float_max(flow_count_estimate, 1.);
            flow_count_estimate = float_min(flow_count_estimate, self.p_ub_flow_count as f64);
            let target_cwnd =
                self.s_cwnd * flow_count_estimate / self.s_round_communicated_flow_count;
            next_cwnd = (1. - self.p_cwnd_averaging_factor) * prev_cwnd
                + self.p_cwnd_averaging_factor * target_cwnd;

            log_bandwidth_estimate = Some(bandwidth_estimate);
            log_flow_count_estimate = Some(flow_count_estimate);
            log_target_cwnd = Some(target_cwnd);
        }

        next_cwnd = float_min(next_cwnd, self.p_cwnd_clamp_high * prev_cwnd);
        next_cwnd = float_max(next_cwnd, prev_cwnd / self.p_cwnd_clamp_low);
        next_cwnd = float_max(next_cwnd, self.p_lb_cwnd);

        self.log_cwnd_update(
            now,
            prev_cwnd,
            probe_excess_qdel,
            log_bandwidth_estimate,
            log_flow_count_estimate,
            log_target_cwnd,
            next_cwnd,
        );
        self.s_cwnd = f64::ceil(next_cwnd);
    }

    fn start_probe(&mut self, now: Time) {
        self.reset_probe_state();
        self.s_probe_ongoing = true;
        self.s_probe_initiated_end = false;
        self.s_probe_start_time = None; // TODO: should this be now or the time we have transmitted the first seq of probe? I think first seq of probe being sent.
        self.s_probe_cwnd_before = self.s_cwnd;
        self.s_probe_min_qdel_before = self.s_slot_min_qdel.unwrap();
        self.s_probe_min_qdel_during = None;
        let round_communicated_flow_count = float_max(self.s_round_communicated_flow_count, 1.);
        let s_excess_amount = f64::ceil(
            self.p_probe_multiplier
                * self.s_round_max_cruise_rate
                * round_communicated_flow_count
                * self.p_ub_jitter.secs(),
        );
        assert!(s_excess_amount > 0.);
        self.s_probe_excess_amount = s_excess_amount as u64;
        self.s_cwnd = self.s_probe_cwnd_before + (self.s_probe_excess_amount as f64);
        self.s_probe_last_seq = None;
        self.s_probe_drain_last_seq = None;

        self.s_probe_first_seq = Some(self.s_tot_tx + f64::ceil(self.s_cwnd) as u64);
        // The first seq is our estimation of when the inflight would have
        // increased to the new cwnd. If cwnd is doubled, and our pacing rate
        // is twice cwnd/RTT, then that after sending a cwnd worth of packets
        // we would have filled inflight as we effectively send 2 packets per
        // ack. If cwnd is less than doubled then we would have increased
        // inflight sooner than an RTT. Other flow will not know this (to set
        // accurate slot size) so conservatively assuming that inflight only
        // increases after cwnd sent, does not affect convergence time.

        // Ideally we can set it based on measured inflight. This is good to
        // verify at least, but our slot time will be in RTTs anyway.

        // TODO: Is this really true? ^^ Maybe we can burst out packets if that
        // does not affect self induced jitter.

        // TODO: Double check probing sequences so that we can be sure of
        // excess delay. Also double check the slot duration based on the
        // sequence change.
    }

    fn reset_probe_state(&mut self) {
        self.s_probe_ongoing = false;
        self.s_probe_initiated_end = false;
        self.s_probe_start_time = None;
        self.s_probe_cwnd_before = self.p_lb_cwnd;
        self.s_probe_min_qdel_before = Time::from_millis(0);
        self.s_probe_first_seq = None;
        self.s_probe_last_seq = None;
        self.s_probe_drain_last_seq = None;
        self.s_probe_min_qdel_during = None;
        self.s_probe_excess_amount = 0;
    }

    fn check_update_cruise_state_and_rate(&mut self, now: Time, ack: SeqNum) {
        if self.s_round_cruise_records.is_empty() {
            self.add_cruise_entry(now, ack);
        }

        let last_record: &mut CruiseRecord = self.s_round_cruise_records.last_mut().unwrap();
        last_record.probe_ongoing = last_record.probe_ongoing || self.s_probe_ongoing;

        if self.cruise_measurement_elapsed(now) {
            self.fill_cruise_entry(now, ack);
            self.log_filled_cruise_entry();
            self.update_cruise_rate();
            self.add_cruise_entry(now, ack);
        }
    }

    fn cruise_measurement_elapsed(&self, now: Time) -> bool {
        // now >= self.s_round_cruise_records.last().unwrap().start_time
        //     + self.p_cruise_measurement_duration

        let last_record = self.s_round_cruise_records.last().unwrap();
        // RTT elapsed (the packet we sent after cruise start has been acked)
        // NOTE: This ACK will cause a new packet to be txed, we wait for its
        // ACK to arrive.
        last_record.start_tot_tx + 1 <= self.s_tot_rx + self.s_tot_ld
    }

    fn fill_cruise_entry(&mut self, now: Time, ack: SeqNum) {
        self.s_round_cruise_records.last_mut().unwrap().fill_end(
            now,
            self.s_tot_tx,
            self.s_tot_rx,
            self.s_tot_ld,
            ack,
        );
    }

    fn log_filled_cruise_entry(&self) {
        let last_record = self.s_round_cruise_records.last().unwrap();
        self.cruise_metric.as_ref().unwrap().borrow_mut().log(last_record.to_row());
    }

    fn add_cruise_entry(&mut self, now: Time, ack: SeqNum) {
        self.s_round_cruise_records.push(CruiseRecord::new(
            now,
            self.s_tot_tx,
            self.s_tot_rx,
            self.s_tot_ld,
            ack,
            self.s_probe_ongoing,
        ));
    }

    fn update_cruise_rate(&mut self) {
        let last_record = self.s_round_cruise_records.last().unwrap();
        self.s_latest_cruise_rate = last_record.ack_rate.unwrap();
        if !last_record.probe_ongoing {
            self.s_round_max_cruise_rate =
                float_max(self.s_round_max_cruise_rate, self.s_latest_cruise_rate);
        }
    }

    fn update_communicated_flow_count(&mut self, now: Time, rtt: Time) {
        // ? should we just use min queueing delay. I guess the min queueing
        // delay is over a slot, we want min over the round.
        let qdel = rtt - self.s_min_rtt;
        // self.s_queueing_delay_records.push(DelayRecord {
        //     time: now,
        //     queueing_delay,
        // });

        // min delay over the round (not just slot)
        let this_flow_count = (qdel.micros() as f64) / (self.p_contract_min_delay.micros() as f64);
        self.s_round_communicated_flow_count =
            float_min(self.s_round_communicated_flow_count, this_flow_count);

        // Clamps
        // If we keep these then a single flow has no incentive to create delay,
        // likewise, there is no incentive to reduce delay when flow count is
        // 10.

        // self.s_round_communicated_flow_count = float_max(self.s_round_communicated_flow_count, 1.);

        // self.s_round_communicated_flow_count = float_min(
        //     self.s_round_communicated_flow_count,
        //     self.p_ub_flow_count as f64,
        // );
    }

    fn slot_ended(&self, now: Time) -> bool {
        // Slot duration is max {max rtprop + queueing delay + probe_duration, T}

        // ? We want queueing delay measurement, so that all flows have roughly
        // similar slot sizes. Currently taken min queueing delay of latest
        // slot.
        let mut slot_duration = self.p_probe_duration + self.p_ub_rtprop * 3 + self.s_slot_max_qdel * 3;
        // ^^ Roughly 2 rtts + probe duration. max_rtprop + max_qdel is ub on
        // rtt.
        slot_duration = std::cmp::max(slot_duration, self.p_cruise_measurement_duration);

        now >= self.s_slot_start_time + slot_duration
    }

    fn update_slot_state(&mut self, _now: Time, rtt: Time) {
        let queueing_delay = rtt - self.s_min_rtt;
        self.s_slot_max_qdel = std::cmp::max(self.s_slot_max_qdel, queueing_delay);

        if self.s_slot_min_qdel.is_none() {
            self.s_slot_min_qdel = Some(queueing_delay);
        }
        self.s_slot_min_qdel = Some(std::cmp::min(self.s_slot_min_qdel.unwrap(), queueing_delay));
    }

    fn start_new_slot(&mut self, now: Time, rtt: Time) {
        // ? Should we keep some state from previous slot for the next slot?
        self.s_slot_start_time = now;
        self.s_slot_max_qdel = rtt - self.s_min_rtt;
        self.s_slot_min_qdel = Some(rtt - self.s_min_rtt);
        self.s_round_slots_till_now += 1;
    }

    fn round_ended(&self) -> bool {
        self.s_round_slots_till_now >= self.p_ub_flow_count * self.p_slot_load_factor as u64
    }

    fn reset_round_state(&mut self) {
        // Since we do not probe on round end, and are guaranteed to have a
        // cruise after round end and before probe, we can clear all the round
        // state.

        self.s_round_slots_till_now = 0; // count
        self.s_round_communicated_flow_count = self.p_ub_flow_count as f64; // min
        self.s_round_cruise_records.clear();
        self.s_round_max_cruise_rate = 0.; // max

        // self.s_queueing_delay_records.clear();  // min

        // ? Currently choosing to clear cruise records altogether. This will
        // force one cruise measurement before probe. Should we instead keep some
        // cruise records from previous round?

        // let last_record = *self.s_cruise_records.last().unwrap();
        // self.s_cruise_rate_this_round = last_record.get_ack_rate();  // max
        // self.s_cruise_records.clear();
        // self.s_cruise_records.push(last_record);
    }

    fn should_start_probe(&mut self) -> bool {
        self.rng.gen_bool(self.p_probe_probability)

        // // Deterministic probes that do not collide
        // let flow_id: u64 = self.name.parse().unwrap();
        // self.s_round_slots_till_now == flow_id
    }

    fn log_slot_metric(&self, now: Time, cruise_ended: bool, probe_ended: bool, round_ended: bool) {
        self.slot_metric.as_ref().unwrap().borrow_mut().log(
            SlotMetric {
                start_time: self.s_slot_start_time,
                end_time: now,
                duration: now - self.s_slot_start_time,
                min_qdel: self.s_slot_min_qdel.unwrap(),
                max_qdel: self.s_slot_max_qdel,
                communicated_flow_count: self.s_round_communicated_flow_count,
                round_max_cruise_rate: self.s_round_max_cruise_rate,
                latest_cruise_rate: self.s_latest_cruise_rate,
                cruise_happened: cruise_ended,
                probe_happened: probe_ended,
                round_ended,
                cwnd: self.s_cwnd,
            }
            .to_row(),
        );
    }
}

impl Default for NDDProved {
    fn default() -> Self {
        let rng_seed = 42;
        let t_by_d = 4; // T/D, duration of cruise measurement relative to jitter.
        let jitter_belief = Time::from_millis(10);
        let max_rtprop = Time::from_millis(100);
        let slot_load_factor = 3;
        let max_flow_count = 10;
        let min_cwnd = 2.; // packets

        NDDProved {
            name: "".to_string(),
            rng: StdRng::seed_from_u64(rng_seed),
            p_rng_seed: rng_seed,

            metric_registry: None,
            slot_metric: None,
            cwnd_update_metric: None,
            cruise_metric: None,
            ack_metric: None,
            send_metric: None,

            p_cruise_measurement_duration: jitter_belief * t_by_d,
            p_cruise_measurement_duration_multiplier: t_by_d,
            p_cwnd_averaging_factor: 0.5,
            p_cwnd_clamp_high: 1.2,
            p_cwnd_clamp_low: 1.1,
            p_probe_multiplier: 4.,
            p_probe_duration: jitter_belief, // ?
            p_contract_min_delay: Time::from_micros(max_rtprop.micros() / 2), // ? ceil vs floor
            p_slot_load_factor: slot_load_factor,
            p_probe_probability: 1. / ((slot_load_factor as f64) * (max_flow_count as f64)),

            p_ub_flow_count: max_flow_count,
            p_ub_jitter: jitter_belief,
            p_ub_rtprop: max_rtprop,
            p_lb_cwnd: min_cwnd,
            p_lb_intersend_time: Time::from_micros(10),
            // Corresponds to rate of 1/100 pkts per ms or roughly 0.12 Mbps.

            // we only use this for srtt which is independent of hist_period,
            // so any value here is okay.
            s_latest_rtt: Time::from_millis(0),
            s_srtt: RTTWindow::new(Time::from_secs(10)),
            s_min_rtt: max_rtprop,
            s_cwnd: min_cwnd,

            s_tot_tx: 0,
            s_tot_rx: 0,
            s_tot_ld: 0,

            s_slot_start_time: Time::from_micros(0),
            s_slot_max_qdel: Time::from_millis(0),
            s_slot_min_qdel: None,

            s_round_slots_till_now: 0,
            s_round_communicated_flow_count: max_flow_count as f64,

            // s_qdel_records: Vec::new(),
            s_round_max_cruise_rate: 0.,
            s_round_cruise_records: Vec::new(),
            s_latest_cruise_rate: 0.,

            s_probe_ongoing: false,
            s_probe_initiated_end: false,
            s_probe_start_time: None,
            s_probe_cwnd_before: min_cwnd,
            s_probe_min_qdel_before: Time::from_millis(0),
            s_probe_first_seq: None,
            s_probe_last_seq: None,
            s_probe_drain_last_seq: None,
            s_probe_min_qdel_during: None,
            s_probe_excess_amount: 0,
        }
    }
}

// test the rng
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rng() {
        let mut ndd = NDDProved::default();
        let n_exp = 10;
        let n_samples = 30 as u64;
        for exp_id in 0..n_exp {
            let mut positive = 0;
            for sample_id in 0..n_samples {
                let this_sample = ndd.should_start_probe();
                positive += this_sample as u64;
            }
            println!(
                "Exp {}: Positive rate: {} Expected rate: {}",
                exp_id,
                positive as f64 / n_samples as f64,
                ndd.p_probe_probability
            );
        }
    }
}
