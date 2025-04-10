#!/usr/bin/env bash

{
set -xeou pipefail

python plot_seq.py -i ../sim-experiments/bw_estimate_analysis/vanilla-dumbbell/0ack.csv -r 39 --train &
python plot_seq.py -i ../sim-experiments/packet_train_bad/with_120Mbps_same_rtt/0ack.csv -r 39 --train &

python plot_seq.py -i ../sim-experiments/packet_train_bad/without_hop_large_oscillations/1ack.csv -r 30 &
python plot_seq.py -i ../sim-experiments/packet_train_bad/without_hop_large_oscillations/0ack.csv -r 30 &
python plot_seq.py -i ../sim-experiments/packet_train_bad/with_pacing_during_probe_up/0ack.csv -r 30 &
python plot_seq.py -i ../sim-experiments/packet_train_bad/with_pacing_during_probe_up/1ack.csv -r 30 &

wait

mkdir -p ../sim-experiments/plot_all
cp ../sim-experiments/bw_estimate_analysis/vanilla-dumbbell/0ack.pdf ../sim-experiments/plot_all/packet-train-nohop.pdf
cp ../sim-experiments/packet_train_bad/with_120Mbps_same_rtt/0ack.pdf ../sim-experiments/plot_all/packet-train-hop.pdf

cp ../sim-experiments/packet_train_bad/with_pacing_during_probe_up/0ack.pdf ../sim-experiments/plot_all/pacing-short.pdf
cp ../sim-experiments/packet_train_bad/with_pacing_during_probe_up/1ack.pdf ../sim-experiments/plot_all/pacing-long.pdf
cp ../sim-experiments/packet_train_bad/without_hop_large_oscillations/0ack.pdf ../sim-experiments/plot_all/nopacing-short.pdf
cp ../sim-experiments/packet_train_bad/without_hop_large_oscillations/1ack.pdf ../sim-experiments/plot_all/nopacing-long.pdf

}
