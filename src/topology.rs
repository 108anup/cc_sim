use crate::base::*;
use crate::cc;
use crate::config::SenderGroupConfig;
use crate::config::TopoType;
use crate::config::{CCConfig, Config};
use crate::copa;
use crate::copa2;
use crate::ndd;
use crate::ndd_proved;
use crate::ndd_slow;
use crate::simulator::*;
use crate::tracer::Tracer;
use crate::transport::*;

use failure::Error;

pub fn create_topology<'a>(config: &'a Config, tracer: &'a Tracer) -> Result<Scheduler<'a>, Error> {
    match config.topo.topo_type {
        TopoType::Dumbbell => create_dumbbell_topology(config, tracer),
        TopoType::ParkingLot => create_parking_lot_topology(config, tracer),
    }
}

fn get_cca(group_config: &SenderGroupConfig) -> Box<dyn CongestionControl> {
    match group_config.cc {
        CCConfig::Const { cwnd, intersend } => {
            Box::new(cc::Const::new(cwnd, Time::from_micros(intersend)))
        }
        CCConfig::AIMD => Box::new(cc::AIMD::default()),
        CCConfig::InstantCC => Box::new(cc::InstantCC::default()),
        CCConfig::OscInstantCC { k, omega } => Box::new(cc::OscInstantCC::new(k, omega)),
        CCConfig::StableLinearCC { alpha, k } => {
            Box::new(cc::StableLinearCC::new(alpha, k, group_config.delay))
        }
        CCConfig::IncreaseBdpCC => Box::new(cc::IncreaseBdpCC::default()),
        CCConfig::Copa => Box::new(copa::Copa::default()),
        CCConfig::Copa2 => Box::new(copa2::Copa2::new(group_config.delay)),
        CCConfig::NDD => Box::new(ndd::NDD::default()),
        CCConfig::NDDSlow => Box::new(ndd_slow::NDDSlow::default()),
        CCConfig::NDDProved => Box::new(ndd_proved::NDDProved::default()),
    }
}

fn create_flow<'a>(
    group_config: &SenderGroupConfig,
    id: &mut usize,
    objs_to_reg: &mut Vec<Box<dyn NetObj + 'a>>,
    router: &mut Router,
    sched: &mut Scheduler<'a>,
    tracer: &'a Tracer,
    config: &'a Config,
    link_id: usize,
    acker_addrs: &mut Vec<Addr>,
) {
    let mut ccalg = get_cca(group_config);
    let tcp_sender_id = *id;
    let delay_id = tcp_sender_id + 1;
    let agg_id = delay_id + 1;
    let acker_id = agg_id + 1;
    *id = acker_id + 1;

    let acker_addr = sched.next_addr();

    let name = tcp_sender_id.to_string();
    ccalg.init(&name, config.metrics_config_file.clone());

    let sender_addr = sched.next_addr();
    let tcp_sender = TcpSender::new(
        delay_id,
        sender_addr,
        acker_addr,
        ccalg,
        group_config.start_time,
        group_config.tx_length,
        &tracer,
        config,
    );
    let delay = Delay::new(group_config.delay, link_id);

    // Create the acker
    let acker = Acker::new(acker_addr, agg_id);
    acker_addrs.push(acker_addr);

    // Add the aggregator after the acker
    let aggregator = Aggregator::new(group_config.agg_intersend, tcp_sender_id);

    // Add routes
    let port = router.add_port(acker_id);
    router.add_route(acker_addr, port);

    objs_to_reg.push(Box::new(tcp_sender));
    objs_to_reg.push(Box::new(delay));
    objs_to_reg.push(Box::new(aggregator));
    objs_to_reg.push(Box::new(acker));
}

fn create_parking_lot_topology<'a>(
    config: &'a Config,
    tracer: &'a Tracer,
) -> Result<Scheduler<'a>, Error> {
    let mut sched = Scheduler::default();

    assert_eq!(
        config.topo.sender_groups.len(),
        1,
        "Only one sender group supported for parking lot topology"
    );
    let group_config = &config.topo.sender_groups[0];
    let num_senders = group_config.num_senders;
    assert!(num_senders >= 2);
    let hops = num_senders - 1;

    let mut id = sched.next_obj_id(); // next id that can be consumed

    // Create hops
    let mut links = Vec::new();
    let mut routers = Vec::new();
    let mut link_ids = Vec::new();
    let mut router_ids = Vec::new();
    for _ in 0..hops {
        link_ids.push(id);
        let router_id = id + 1;
        router_ids.push(router_id);
        let link_trace = LinkTrace::from_config(&config.topo.link, config)?;
        links.push(Link::new(
            link_trace,
            config.topo.bufsize,
            router_id,
            &tracer,
            &config,
        ));
        routers.push(Router::new(sched.next_addr()));
        id = router_id + 1;
    }

    // Create the senders
    let mut acker_addrs = Vec::new();
    let mut objs_to_reg = Vec::<Box<dyn NetObj + 'a>>::new();
    // The first flow (flow 0 sees all the hops)
    create_flow(
        group_config,
        &mut id,
        &mut objs_to_reg,
        routers.last_mut().unwrap(),
        &mut sched,
        tracer,
        config,
        link_ids[0],
        &mut acker_addrs,
    );
    for i in 1..num_senders {
        create_flow(
            group_config,
            &mut id,
            &mut objs_to_reg,
            &mut routers[i-1],
            &mut sched,
            tracer,
            config,
            link_ids[i-1],
            &mut acker_addrs,
        );
    }

    // Tell the routers to forward packets from flow 0 to the next hop link.
    for i in 0..hops-1 {
        let port = routers[i].add_port(link_ids[i+1]);
        routers[i].add_route(acker_addrs[0], port);
    }

    while links.len() > 0 {
        let link = links.remove(0);
        let router = routers.remove(0);
        sched.register_obj(Box::new(link));
        sched.register_obj(Box::new(router));
    }
    assert!(routers.is_empty());

    for obj in objs_to_reg {
        sched.register_obj(obj);
    }

    Ok(sched)
}

/// Creates topology specified in Config and returns a Scheduler (with appropriate NetObjects). The
/// base topology is as follows (tcp_sender -> delay) -> link -> router --..--> ackers -->
/// aggregator --> back to corresponding senders
fn create_dumbbell_topology<'a>(
    config: &'a Config,
    tracer: &'a Tracer,
) -> Result<Scheduler<'a>, Error> {
    let mut sched = Scheduler::default();

    let link_id = sched.next_obj_id();
    let router_id = link_id + 1;

    // Create bottleneck
    let link_trace = LinkTrace::from_config(&config.topo.link, config)?;
    let link = Link::new(link_trace, config.topo.bufsize, router_id, &tracer, &config);
    let mut router = Router::new(sched.next_addr());

    // Register the core objects. Remember to do it in the same order as the ids
    sched.register_obj(Box::new(link));
    //sched.register_obj(Box::new(acker));

    // List of objects we need to register, in the order we should register them. Before
    // registering these, we'll register router
    let mut objs_to_reg = Vec::<Box<dyn NetObj + 'a>>::new();

    // Now create the senders
    for group_config in &config.topo.sender_groups {
        for _ in 0..group_config.num_senders {
            let mut ccalg = get_cca(group_config);

            // Decide everybody's ids
            let tcp_sender_id = router_id + 1 + objs_to_reg.len();
            let delay_id = tcp_sender_id + 1;
            let agg_id = delay_id + 1;
            let acker_id = agg_id + 1;

            let acker_addr = sched.next_addr();

            let name = tcp_sender_id.to_string();
            ccalg.init(&name, config.metrics_config_file.clone());

            // Create the sender and its delay module
            let sender_addr = sched.next_addr();
            let tcp_sender = TcpSender::new(
                delay_id,
                sender_addr,
                acker_addr,
                ccalg,
                group_config.start_time,
                group_config.tx_length,
                &tracer,
                config,
            );
            let delay = Delay::new(group_config.delay, link_id);

            // Create the acker
            let acker = Acker::new(acker_addr, agg_id);

            // Add the aggregator after the acker
            let aggregator = Aggregator::new(group_config.agg_intersend, tcp_sender_id);

            // Add routes
            let port = router.add_port(acker_id);
            router.add_route(acker_addr, port);

            objs_to_reg.push(Box::new(tcp_sender));
            objs_to_reg.push(Box::new(delay));
            objs_to_reg.push(Box::new(aggregator));
            objs_to_reg.push(Box::new(acker));
        }
    }

    // First register the router, which we couldn't register earlier since we were still adding
    // routes
    sched.register_obj(Box::new(router));
    // Register all sender-side objects with the scheduler
    for obj in objs_to_reg {
        sched.register_obj(obj);
    }

    Ok(sched)
}
