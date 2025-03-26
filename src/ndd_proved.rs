use std::cell::RefCell;
use std::fmt::Display;
use std::rc::Rc;

use num::Float;
use rand::prelude::*;
use rand::rngs::StdRng;
use rand_seeder::Seeder;
use serde::{Deserialize, Serialize};

use crate::metrics::{CsvMetric, CsvMetricStruct, MetricConfig, MetricRegistry};
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

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(default)]
pub struct NDDParams {
    p_rng_seed: u64,

    f_wait_rtt_after_probe: bool,
    f_deterministic_slot_idx: bool,
    f_probe_wait_in_max_rtts: bool,
    f_probe_duration_max_rtt: bool,
    f_drain_over_rtt: bool,
    f_slot_greater_than_rtprop: bool,

    p_cwnd_averaging_factor: f64,
    p_cwnd_clamp_high: f64,
    p_cwnd_clamp_low: f64,
    p_probe_multiplier: f64,
    p_probe_duration: Time,
    p_contract_min_qdel: Time,
    p_slots_per_round: u64,
    p_probe_wait_rtts: u64,

    p_ub_flow_count: u64,
    p_ub_rtterr: Time,
    p_ub_rtprop: Time,
    p_lb_cwnd_pkts: f64,
    p_lb_intersend_time: Time,
}

impl Default for NDDParams {
    fn default() -> Self {
        NDDParams {
            p_rng_seed: 42,

            f_wait_rtt_after_probe: true,
            f_deterministic_slot_idx: false,
            f_probe_wait_in_max_rtts: true,
            f_probe_duration_max_rtt: true,
            f_drain_over_rtt: true,
            f_slot_greater_than_rtprop: true,

            p_cwnd_averaging_factor: 1.,
            p_cwnd_clamp_high: 1.3,
            p_cwnd_clamp_low: 1.3,
            p_probe_multiplier: 4.,
            p_probe_duration: Time::from_millis(10),
            p_contract_min_qdel: Time::from_millis(10),
            p_slots_per_round: 10,
            p_probe_wait_rtts: 2,

            p_ub_flow_count: 10,
            p_ub_rtterr: Time::from_millis(10),
            p_ub_rtprop: Time::from_millis(100),
            p_lb_cwnd_pkts: 2.,
            p_lb_intersend_time: Time::from_micros(10),
        }
    }
}

pub struct NDDProved {
    name: String,
    s_rng: StdRng,
    p_rng_seed: u64,

    // -------------------------------------------------------------------------
    // METRICS
    m_registery: Option<MetricRegistry>,
    m_slot: Option<Rc<RefCell<CsvMetric>>>,
    m_cwnd_update: Option<Rc<RefCell<CsvMetric>>>,
    m_cruise: Option<Rc<RefCell<CsvMetric>>>,
    m_ack: Option<Rc<RefCell<CsvMetric>>>,
    m_send: Option<Rc<RefCell<CsvMetric>>>,
    m_cwnd_event: Option<Rc<RefCell<CsvMetric>>>,

    // -------------------------------------------------------------------------
    // FEATURES
    f_wait_rtt_after_probe: bool,
    f_deterministic_slot_idx: bool,
    f_probe_wait_in_max_rtts: bool,
    f_probe_duration_max_rtt: bool,
    f_drain_over_rtt: bool,
    f_slot_greater_than_rtprop: bool,

    // -------------------------------------------------------------------------
    // PARAMETERS
    // Design parameters (derived from objectives)
    p_cwnd_averaging_factor: f64, // alpha
    p_cwnd_clamp_high: f64,       // delta1
    p_cwnd_clamp_low: f64,        // delta2
    p_probe_multiplier: f64,      // gamma
    p_probe_duration: Time,       // This can really be anything
    p_contract_min_qdel: Time,
    p_slots_per_round: u64,
    p_probe_wait_rtts: u64,

    // Prior belief of network parameters (upper (ub) and lower (lb) bounds)
    p_ub_flow_count: u64,
    p_ub_rtterr: Time, // D
    p_ub_rtprop: Time,
    p_lb_cwnd_pkts: f64, // packets
    p_lb_intersend_time: Time,

    // -------------------------------------------------------------------------
    // STATE
    s_cwnd: f64, // packets
    s_min_rtprop: Time,

    // CCAC style monotonically increasing packet counters.
    s_tot_tx: u64,
    s_tot_rx: u64,
    s_tot_ld: u64,
    s_latest_rtt: Time,
    // ? I don't think CCAC formulation allowed false positives in loss
    // detection. So this might not work correctly.

    // Slow start
    s_ss_done: bool,
    s_ss_end_initiated: bool,
    s_ss_last_seq: u64,

    // Collision slot state (Slot level)
    s_slot_start_time: Time,
    s_slot_max_qdel: Time,         // Upper bound on E/C
    s_slot_min_qdel: Option<Time>, // For computing excess delay when probing.

    // State for N_R estimate (Round level)
    s_round_slots_till_now: u64,
    s_round_communicated_flow_count: f64,

    // Cruise state (get record/sample every T time (independent of slot time)
    s_round_max_cruise_rate: f64, // packets per second
    s_round_cruise_records: Vec<CruiseRecord>,
    s_round_probed: bool,
    s_round_probe_slot_idx: u64,
    s_latest_cruise_rate: f64, // packets per second

    // Probe state (for the slot in which we probe, the probe may be smaller
    // than the slot duration)
    s_probe_ongoing: bool,
    s_probe_initiated_end: bool,
    s_probe_first_time: Option<Time>,
    s_probe_start_time: Option<Time>,
    s_probe_cwnd_before: f64,
    s_probe_min_qdel_before: Time,
    s_probe_min_qdel_during: Option<Time>,
    s_probe_start_seq: Option<u64>,
    s_probe_inflightmatch_seq: Option<u64>,
    s_probe_first_seq: Option<u64>,
    s_probe_last_seq: Option<u64>,
    s_probe_excess_amount: u64, // packets
}

impl Display for NDDProved {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // output all the parameters
        writeln!(f, "NDDProved parameters:")?;
        writeln!(f, "p_rng_seed: {}", self.p_rng_seed)?;
        writeln!(
            f,
            "p_cwnd_averaging_factor: {}",
            self.p_cwnd_averaging_factor
        )?;
        writeln!(f, "p_cwnd_clamp_high: {}", self.p_cwnd_clamp_high)?;
        writeln!(f, "p_cwnd_clamp_low: {}", self.p_cwnd_clamp_low)?;
        writeln!(f, "p_probe_multiplier: {}", self.p_probe_multiplier)?;
        writeln!(f, "p_probe_duration: {}", self.p_probe_duration)?;
        writeln!(f, "p_contract_min_delay: {}", self.p_contract_min_qdel)?;
        writeln!(f, "p_slots_per_round: {}", self.p_slots_per_round)?;
        writeln!(f, "p_ub_flow_count: {}", self.p_ub_flow_count)?;
        writeln!(f, "p_ub_rtterr: {}", self.p_ub_rtterr)?;
        writeln!(f, "p_ub_rtprop: {}", self.p_ub_rtprop)?;
        writeln!(f, "p_lb_cwnd: {}", self.p_lb_cwnd_pkts)?;
        writeln!(f, "p_lb_intersend_time: {}", self.p_lb_intersend_time)?;
        Ok(())
    }
}

#[derive(Serialize, Default)]
enum CwndEvent {
    #[default]
    SlowStart,
    SlowStartEnd,
    ProbeGain,
    ProbeDrain,
    ProbeUpdate,
    RProbeDrain,
    RProbeRefill,
}

#[derive(Serialize, Default)]
struct CwndEventMetric {
    time: Time,
    event: CwndEvent,
    cwnd: f64,
}

impl CsvMetricStruct for CwndEventMetric {}

#[derive(Serialize, Default)]
struct CwndUpdateMetric {
    now: Time,
    probe_cwnd_before: f64,
    probe_min_qdel_before: Time,
    probe_min_qdel_during: Time,
    s_probe_start_seq: Option<u64>,
    s_probe_inflightmatch_seq: Option<u64>,
    s_probe_first_seq: Option<u64>,
    s_probe_last_seq: Option<u64>,
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

        self.log_ack_metric(now, cum_ack, rtt);

        self.s_min_rtprop = std::cmp::min(self.s_min_rtprop, rtt);
        // TODO: timeout min_rtt estimate

        // ? split into measurement updates and cwnd action?
        self.check_update_cruise_state_and_rate(now, cum_ack);
        self.update_probe_delay_if_allowed(cum_ack, rtt);
        self.update_communicated_flow_count(now, rtt);
        self.update_slot_state(now, rtt);
        self.update_probe_state(now);

        // ? We from from old state to next state. Since it is multi-variable,
        // we update the state variable by variable, but sometimes we need
        // access to old state variable. Currently, we have carefully chosen
        // the order to avoid this. Ideally, we can keep old and next state and
        // swap them at the end.

        if !self.s_ss_done {
            self.slow_start(now, rtt);
            return;
        }

        if self.s_probe_ongoing && self.should_initiate_probe_end(now, rtt) {
            self.initiate_probe_end(now);
        }

        let probe_ended = self.s_probe_ongoing && self.should_end_probe(cum_ack);
        let cruise_ended = !self.s_probe_ongoing && self.slot_ended(now);

        if probe_ended || cruise_ended {
            // slot ended
            let round_ended = self.round_ended();
            self.log_slot_metric(now, cruise_ended, probe_ended, round_ended);

            if self.should_end_probe(cum_ack) {
                self.update_cwnd_after_probe(now);
                self.reset_probe_state();
            }

            // TODO: we can follow convention that round ends after probe so
            // that we maximize information gathering?

            if round_ended {
                self.reset_round_state()
            }

            // Round may have ended or a probe slow may have ended. We
            // increment slots_till_now at the end, so if there has been at
            // least one slot before for a total of at least 2 (including this
            // one), then we are sure at least one cruise happened
            if self.s_round_slots_till_now >= 1 && !self.s_round_probed && self.should_start_probe()
            {
                self.s_round_probed = true;
                self.start_probe(now);
            }

            self.start_new_slot(now, rtt);
        }
    }

    fn on_send(&mut self, now: Time, _seq_num: SeqNum, _uid: PktId) {
        self.s_tot_tx += 1;

        //if self.s_probe_ongoing
        //    && self.s_probe_start_time.is_none()
        //    && self.s_tot_tx >= self.s_probe_first_seq.unwrap()
        //{
        //    self.s_probe_start_time = Some(now);
        //}

        self.log_send_metric(now);
    }

    fn get_cwnd(&mut self) -> u64 {
        f64::ceil(self.s_cwnd) as u64
    }

    fn get_intersend_time(&mut self) -> Time {
        if self.s_min_rtprop.micros() == u64::MAX {
            self.p_lb_intersend_time
        } else {
            // std::cmp::max(
            //     self.p_lb_intersend_time,
            //     Time::from_micros((self.s_latest_rtt.micros() as f64 / (self.s_cwnd * 2.)) as u64),
            // )
            std::cmp::max(
                self.p_lb_intersend_time,
                Time::from_micros((self.s_min_rtprop.micros() as f64 / (self.s_cwnd * 2.)) as u64),
            )
            // self.p_lb_intersend_time
        }
    }

    fn on_timeout(&mut self) {
        self.s_cwnd = self.p_lb_cwnd_pkts;
    }

    fn init(&mut self, name: &str, metric_config: Option<MetricConfig>) {
        self.name = name.to_string();
        self.init_metrics(metric_config);
        self.reset_round_state();
        self.reset_probe_state();
        // self.rng = StdRng::seed_from_u64(self.p_rng_seed);
        let unique_str = &(name.to_owned() + self.p_rng_seed.to_string().as_str());
        let seed = Seeder::from(unique_str).make_seed::<[u8; 32]>();
        self.s_rng = StdRng::from_seed(seed);

        println!("Initialized NDDProved {}", name);
        println!("{}", self);
    }

    fn finish(&self) {
        if let Some(metric_registry) = &self.m_registery {
            metric_registry.finish();
        }
    }
}

impl NDDProved {
    fn slow_start(&mut self, now: Time, rtt: Time) {
        let last_snd_seq = self.s_tot_tx;
        let last_recv_seq = self.s_tot_rx + self.s_tot_ld;

        let should_init_ss_end =
            rtt > (self.s_min_rtprop + self.p_contract_min_qdel + self.p_ub_rtterr);
        let ss_ended = self.s_ss_last_seq > 0 && last_recv_seq >= self.s_ss_last_seq;

        if !self.s_ss_end_initiated {
            if !should_init_ss_end {
                self.s_cwnd += 1.;
                self.log_cwnd_event(now, CwndEvent::SlowStart);
            } else {
                self.s_cwnd /= 2.;
                self.s_cwnd = float_max(self.s_cwnd, self.p_lb_cwnd_pkts);
                self.log_cwnd_event(now, CwndEvent::SlowStartEnd);
                self.s_ss_end_initiated = true;
                self.s_ss_last_seq = last_snd_seq;
                // This is the last seq to experience high RTT, we need to ignore the RTT of this
                // seq when computing round min RTT.
            }
        } else {
            #[allow(clippy::collapsible_if)]
            if ss_ended {
                self.s_ss_done = true;
                self.reset_round_state();
                self.start_new_slot(now, rtt);
            } else {
                // ss end initiated but not yet ended. do nothing.
            }
        }
    }

    fn init_metrics(&mut self, metric_config_: Option<MetricConfig>) {
        if let Some(metric_config) = metric_config_ {
            self.m_registery = Some(MetricRegistry::new(metric_config));
        }
        self.m_slot = self
            .m_registery
            .as_mut()
            .unwrap()
            .register_csv_metric(&(self.name.to_owned() + "slot"), SlotMetric::get_columns());
        self.m_cwnd_update = self.m_registery.as_mut().unwrap().register_csv_metric(
            &(self.name.to_owned() + "cwnd_update"),
            CwndUpdateMetric::get_columns(),
        );
        self.m_cruise = self.m_registery.as_mut().unwrap().register_csv_metric(
            &(self.name.to_owned() + "cruise"),
            CruiseRecord::get_columns(),
        );
        self.m_ack = self
            .m_registery
            .as_mut()
            .unwrap()
            .register_csv_metric(&(self.name.to_owned() + "ack"), AckRecord::get_columns());
        self.m_send = self
            .m_registery
            .as_mut()
            .unwrap()
            .register_csv_metric(&(self.name.to_owned() + "send"), SendRecord::get_columns());
        self.m_cwnd_event = self.m_registery.as_mut().unwrap().register_csv_metric(
            &(self.name.to_owned() + "cwnd_event"),
            CwndEventMetric::get_columns(),
        );
    }

    fn log_cwnd_event(&self, now: Time, cwnd_event: CwndEvent) {
        self.m_cwnd_event.as_ref().unwrap().borrow_mut().log(
            CwndEventMetric {
                time: now,
                event: cwnd_event,
                cwnd: self.s_cwnd,
            }
            .to_row(),
        );
    }

    fn log_send_metric(&self, now: Time) {
        self.m_send.as_ref().unwrap().borrow_mut().log(
            SendRecord {
                time: now,
                tot_tx: self.s_tot_tx,
                tot_rx: self.s_tot_rx,
                tot_ld: self.s_tot_ld,
                probe_ongoing: self.s_probe_ongoing,
                cwnd: self.s_cwnd,
                inflight: self.s_tot_tx - self.s_tot_rx - self.s_tot_ld,
            }
            .to_row(),
        )
    }

    fn log_ack_metric(&self, now: Time, cum_ack: SeqNum, rtt: Time) {
        self.m_ack.as_ref().unwrap().borrow_mut().log(
            AckRecord {
                time: now,
                tot_tx: self.s_tot_tx,
                tot_rx: self.s_tot_rx,
                tot_ld: self.s_tot_ld,
                cum_ack,
                rtt,
                probe_ongoing: self.s_probe_ongoing,
                is_part_of_excess_duration: self.s_probe_ongoing
                    && self.is_ack_part_of_excess_duration(cum_ack),
                cwnd: self.s_cwnd,
                inflight: self.s_tot_tx - self.s_tot_rx - self.s_tot_ld,
            }
            .to_row(),
        );
    }

    fn should_initiate_probe_end(&self, now: Time, rtt: Time) -> bool {
        let last_snd_seq = self.s_tot_tx;
        !self.s_probe_initiated_end
            && self.s_probe_last_seq.is_some()
            && last_snd_seq >= self.s_probe_last_seq.unwrap()
    }

    fn should_end_probe(&self, ack: SeqNum) -> bool {
        let last_recv_seq = self.s_tot_rx + self.s_tot_ld;
        self.s_probe_initiated_end && last_recv_seq >= self.s_probe_last_seq.unwrap()
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
        if self.s_probe_ongoing && self.is_ack_part_of_excess_duration(_ack) {
            // update excess delay
            let delay = rtt - self.s_min_rtprop;
            if self.s_probe_min_qdel_during.is_none() {
                self.s_probe_min_qdel_during = Some(delay);
            }
            self.s_probe_min_qdel_during =
                Some(std::cmp::min(self.s_probe_min_qdel_during.unwrap(), delay));
        }
    }

    fn initiate_probe_end(&mut self, now: Time) {
        //self.s_probe_last_seq = Some(std::cmp::max(
        //    self.s_probe_first_seq.unwrap(),
        //    self.s_tot_tx,
        //));
        // The max ensures that even if probe duration is very small (if probe
        // rate too low), the probe contains at least 1 pkt which experiences
        // full delay.
        self.s_cwnd = self.s_probe_cwnd_before;
        self.s_probe_initiated_end = true;
        //self.s_probe_drain_last_seq =
        //    Some(self.s_probe_last_seq.unwrap() + (f64::ceil(self.s_cwnd) as u64));
        self.log_cwnd_event(now, CwndEvent::ProbeDrain);
    }

    #[allow(clippy::too_many_arguments)]
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
        self.m_cwnd_update.as_ref().unwrap().borrow_mut().log(
            CwndUpdateMetric {
                now,
                probe_cwnd_before,
                probe_min_qdel_before: self.s_probe_min_qdel_before,
                probe_min_qdel_during: self.s_probe_min_qdel_during.unwrap(),
                s_probe_start_seq: self.s_probe_start_seq,
                s_probe_inflightmatch_seq: self.s_probe_inflightmatch_seq,
                s_probe_first_seq: self.s_probe_first_seq,
                s_probe_last_seq: self.s_probe_last_seq,
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

        if self.s_probe_min_qdel_before < self.s_probe_min_qdel_during.unwrap()
            && self.s_round_communicated_flow_count > 0.
        {
            probe_excess_qdel =
                self.s_probe_min_qdel_during.unwrap() - self.s_probe_min_qdel_before;
            let bandwidth_estimate = (self.s_probe_excess_amount as f64) / probe_excess_qdel.secs(); // packets per second
            let mut flow_count_estimate = bandwidth_estimate / self.s_round_max_cruise_rate;
            flow_count_estimate = float_max(flow_count_estimate, 1.);
            flow_count_estimate = float_min(flow_count_estimate, self.p_ub_flow_count as f64);
            // let target_cwnd =
            //     self.s_cwnd * flow_count_estimate / self.s_round_communicated_flow_count;
            let target_cwnd = self.s_cwnd
                * (self.s_min_rtprop.secs()
                    + self.p_contract_min_qdel.secs() * flow_count_estimate)
                / (self.s_min_rtprop.secs()
                    + self.p_contract_min_qdel.secs() * self.s_round_communicated_flow_count);
            next_cwnd = (1. - self.p_cwnd_averaging_factor) * prev_cwnd
                + self.p_cwnd_averaging_factor * target_cwnd;

            log_bandwidth_estimate = Some(bandwidth_estimate);
            log_flow_count_estimate = Some(flow_count_estimate);
            log_target_cwnd = Some(target_cwnd);
        }

        next_cwnd = float_min(next_cwnd, self.p_cwnd_clamp_high * prev_cwnd);
        next_cwnd = float_max(next_cwnd, prev_cwnd / self.p_cwnd_clamp_low);
        next_cwnd = float_max(next_cwnd, self.p_lb_cwnd_pkts);

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
        self.log_cwnd_event(now, CwndEvent::ProbeUpdate);
    }

    fn update_probe_state(&mut self, now: Time) {
        if !self.s_probe_ongoing {
            return;
        }

        let last_recv_seq = self.s_tot_rx + self.s_tot_ld;
        let last_snd_seq = self.s_tot_tx;

        let max_rtprop = std::cmp::max(self.s_min_rtprop, self.p_ub_rtprop);
        let max_rtt = max_rtprop + self.s_slot_max_qdel;
        let mut probe_duration = self.p_probe_duration;
        if self.f_probe_duration_max_rtt {
            probe_duration = max_rtt;
        }

        let wait_time = max_rtt * self.p_probe_wait_rtts;
        let wait_until = self.s_probe_start_time.unwrap() + wait_time;

        if self.s_probe_inflightmatch_seq.is_none() {
            if last_recv_seq >= self.s_probe_start_seq.unwrap() {
                self.s_probe_inflightmatch_seq = Some(last_snd_seq+1);
                if !self.f_wait_rtt_after_probe {
                    self.s_probe_first_seq = Some(last_snd_seq+1);
                    self.s_probe_first_time = Some(now);
                }
            }
        } else if self.s_probe_first_seq.is_none() {
            if last_recv_seq >= self.s_probe_inflightmatch_seq.unwrap() {
                #[allow(clippy::collapsible_if)]
                if !self.f_probe_wait_in_max_rtts || now >= wait_until {
                    self.s_probe_first_seq = Some(last_snd_seq+1);
                    self.s_probe_first_time = Some(now);
                }
            }
        } else if self.s_probe_last_seq.is_none() {
            #[allow(clippy::collapsible_if)]
            if now > self.s_probe_first_time.unwrap() + probe_duration {
                self.s_probe_last_seq =
                    Some(std::cmp::max(self.s_probe_first_seq.unwrap(), last_snd_seq));
                // The max ensures there is at least one packet in [first, last]
            }
        }
    }

    fn start_probe(&mut self, now: Time) {
        self.reset_probe_state();
        self.s_probe_ongoing = true;
        self.s_probe_initiated_end = false;
        self.s_probe_first_time = None; // TODO: should this be now or the time we have transmitted the first seq of probe? I think first seq of probe being sent.
        self.s_probe_start_time = Some(now);
        self.s_probe_cwnd_before = self.s_cwnd;
        self.s_probe_min_qdel_before = self.s_slot_min_qdel.unwrap();
        self.s_probe_min_qdel_during = None;
        self.s_probe_start_seq = Some(self.s_tot_tx+1);
        self.s_probe_inflightmatch_seq = None;
        self.s_probe_first_seq = None;
        self.s_probe_last_seq = None;
        let round_communicated_flow_count = float_max(self.s_round_communicated_flow_count, 1.);
        let s_excess_amount = f64::ceil(
            self.p_probe_multiplier
                * self.s_round_max_cruise_rate
                * round_communicated_flow_count
                * self.p_ub_rtterr.secs(),
        );
        assert!(s_excess_amount > 0.);
        self.s_probe_excess_amount = s_excess_amount as u64;

        self.s_cwnd = self.s_probe_cwnd_before + (self.s_probe_excess_amount as f64);
        self.log_cwnd_event(now, CwndEvent::ProbeGain);

        // self.s_probe_first_seq = Some(self.s_tot_tx + f64::ceil(self.s_cwnd) as u64);
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
        self.s_probe_first_time = None;
        self.s_probe_start_time = None;
        self.s_probe_cwnd_before = self.p_lb_cwnd_pkts;
        self.s_probe_min_qdel_before = Time::from_millis(0);
        self.s_probe_start_seq = None;
        self.s_probe_inflightmatch_seq = None;
        self.s_probe_first_seq = None;
        self.s_probe_last_seq = None;
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
        self.m_cruise
            .as_ref()
            .unwrap()
            .borrow_mut()
            .log(last_record.to_row());
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
            // println!("update {}: {}", self.name, self.s_round_max_cruise_rate);
        }
    }

    fn update_communicated_flow_count(&mut self, now: Time, rtt: Time) {
        // ? should we just use min queueing delay. I guess the min queueing
        // delay is over a slot, we want min over the round.
        let qdel = rtt - self.s_min_rtprop;
        // self.s_queueing_delay_records.push(DelayRecord {
        //     time: now,
        //     queueing_delay,
        // });

        // min delay over the round (not just slot)
        let this_flow_count = (qdel.micros() as f64) / (self.p_contract_min_qdel.micros() as f64);
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
        // similar slot sizes. Currently taken max queueing delay of latest
        // slot.
        let mut max_rtprop = self.p_ub_rtprop;
        if self.f_slot_greater_than_rtprop {
            max_rtprop = std::cmp::max(max_rtprop, self.s_min_rtprop);
        }

        let max_rtt = max_rtprop + self.s_slot_max_qdel;
        let mut probe_duration = self.p_probe_duration;
        if self.f_probe_duration_max_rtt {
            probe_duration = max_rtt;
        }

        let mut slot_duration = probe_duration + max_rtt + self.s_slot_max_qdel;
        if self.f_wait_rtt_after_probe {
            slot_duration = slot_duration + max_rtt;
        }
        if self.f_probe_wait_in_max_rtts {
            slot_duration = std::cmp::max(
                slot_duration,
                probe_duration + max_rtt * self.p_probe_wait_rtts + self.s_slot_max_qdel,
            );
        }

        now >= self.s_slot_start_time + slot_duration
    }

    fn update_slot_state(&mut self, _now: Time, rtt: Time) {
        let queueing_delay = rtt - self.s_min_rtprop;
        self.s_slot_max_qdel = std::cmp::max(self.s_slot_max_qdel, queueing_delay);

        if self.s_slot_min_qdel.is_none() {
            self.s_slot_min_qdel = Some(queueing_delay);
        }
        self.s_slot_min_qdel = Some(std::cmp::min(self.s_slot_min_qdel.unwrap(), queueing_delay));
    }

    fn start_new_slot(&mut self, now: Time, rtt: Time) {
        // ? Should we keep some state from previous slot for the next slot?
        self.s_slot_start_time = now;
        self.s_slot_max_qdel = rtt - self.s_min_rtprop;
        self.s_slot_min_qdel = Some(rtt - self.s_min_rtprop);
        self.s_round_slots_till_now += 1;
    }

    fn round_ended(&self) -> bool {
        self.s_round_slots_till_now >= self.p_slots_per_round
    }

    fn reset_round_state(&mut self) {
        // Since we do not probe on round end, and are guaranteed to have a
        // cruise after round end and before probe, we can clear all the round
        // state.

        self.s_round_slots_till_now = 0; // count
        self.s_round_communicated_flow_count = f64::max_value(); // self.p_ub_flow_count as f64; // min

        self.s_round_max_cruise_rate = 0.; // max
        // println!("reset {}: {}", self.name, self.s_round_max_cruise_rate);
        self.s_round_cruise_records.clear();
        self.s_round_probed = false;
        self.s_round_probe_slot_idx = 1 + self.s_rng.gen_range(0, self.p_slots_per_round);

        // ? Currently choosing to clear cruise records altogether. This will
        // force one cruise measurement before probe. Should we instead keep some
        // cruise records from previous round?

        // let last_record = *self.s_cruise_records.last().unwrap();
        // self.s_cruise_rate_this_round = last_record.get_ack_rate();  // max
        // self.s_cruise_records.clear();
        // self.s_cruise_records.push(last_record);
    }

    fn should_start_probe(&mut self) -> bool {
        if !self.f_deterministic_slot_idx {
            self.s_round_slots_till_now >= self.s_round_probe_slot_idx
        } else {
            // deterministic probes that do not collide

            // if there are n slots, then flow id to slot id mapping is:
            // flow_id, slot_id
            // 0, 0
            // 1, n/2,
            // 2, n/4,
            // 3, 3n/4,
            // 4, n/8,
            // 5, 3n/8, ...

            let flow_id: u64 = self.name.parse().unwrap();
            let pow_2_larger = if flow_id.is_power_of_two() {
                flow_id << 1
            } else {
                flow_id.next_power_of_two()
            };
            let pow_2_leq = pow_2_larger >> 1;
            let mut this_slot = self.p_slots_per_round * (2 * (flow_id - pow_2_leq) + 1) / pow_2_larger;
            this_slot = this_slot % self.p_slots_per_round;

            return self.s_round_slots_till_now >= this_slot
        }
    }

    fn log_slot_metric(&self, now: Time, cruise_ended: bool, probe_ended: bool, round_ended: bool) {
        self.m_slot.as_ref().unwrap().borrow_mut().log(
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

    pub fn new(p: &NDDParams) -> Self {
        Self {
            name: "".to_string(),
            s_rng: StdRng::seed_from_u64(p.p_rng_seed),
            p_rng_seed: p.p_rng_seed,

            m_registery: None,
            m_slot: None,
            m_cwnd_update: None,
            m_cruise: None,
            m_ack: None,
            m_send: None,
            m_cwnd_event: None,

            f_wait_rtt_after_probe: p.f_wait_rtt_after_probe,
            f_deterministic_slot_idx: p.f_deterministic_slot_idx,
            f_probe_wait_in_max_rtts: p.f_probe_wait_in_max_rtts,
            f_probe_duration_max_rtt: p.f_probe_duration_max_rtt,
            f_drain_over_rtt: p.f_drain_over_rtt,
            f_slot_greater_than_rtprop: p.f_slot_greater_than_rtprop,

            p_cwnd_averaging_factor: p.p_cwnd_averaging_factor,
            p_cwnd_clamp_high: p.p_cwnd_clamp_high,
            p_cwnd_clamp_low: p.p_cwnd_clamp_low,
            p_probe_multiplier: p.p_probe_multiplier,
            p_probe_duration: p.p_probe_duration,
            p_contract_min_qdel: p.p_contract_min_qdel,
            p_slots_per_round: p.p_slots_per_round,
            p_probe_wait_rtts: p.p_probe_wait_rtts,

            p_ub_flow_count: p.p_ub_flow_count,
            p_ub_rtterr: p.p_ub_rtterr,
            p_ub_rtprop: p.p_ub_rtprop,
            p_lb_cwnd_pkts: p.p_lb_cwnd_pkts,
            p_lb_intersend_time: p.p_lb_intersend_time,
            // Corresponds to rate of 1/100 pkts per ms or roughly 0.12 Mbps.
            s_cwnd: p.p_lb_cwnd_pkts,
            s_min_rtprop: Time::from_micros(u64::MAX),

            s_tot_tx: 0,
            s_tot_rx: 0,
            s_tot_ld: 0,
            s_latest_rtt: Time::from_micros(0),

            s_ss_done: false,
            s_ss_end_initiated: false,
            s_ss_last_seq: 0,

            s_slot_start_time: Time::from_micros(0),
            s_slot_max_qdel: Time::from_millis(0),
            s_slot_min_qdel: None,

            s_round_slots_till_now: 0,
            s_round_communicated_flow_count: f64::max_value(), // max_flow_count as f64,
            // s_qdel_records: Vec::new(),
            s_round_max_cruise_rate: 0.,
            s_round_cruise_records: Vec::new(),
            s_round_probe_slot_idx: 0, // we will reset round after slow start, so this value does not matter.
            s_round_probed: false,
            s_latest_cruise_rate: 0.,

            s_probe_ongoing: false,
            s_probe_initiated_end: false,
            s_probe_first_time: None,
            s_probe_start_time: None,
            s_probe_cwnd_before: p.p_lb_cwnd_pkts,
            s_probe_min_qdel_before: Time::from_millis(0),
            s_probe_start_seq: None,
            s_probe_inflightmatch_seq: None,
            s_probe_first_seq: None,
            s_probe_last_seq: None,
            s_probe_min_qdel_during: None,
            s_probe_excess_amount: 0,
        }
    }
}
