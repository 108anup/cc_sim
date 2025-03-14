import argparse
import copy
import json
import multiprocessing as mp
import os
import subprocess
import time

SCRIPT_PATH = os.path.dirname(os.path.realpath(__file__))
REPO_PATH = os.path.dirname(SCRIPT_PATH)
SIM_PATH = os.path.join(REPO_PATH, "target/release/cc_sim")
OUT_PATH = os.path.join(REPO_PATH, "outputs")
METRICS_CONFIG_FILE = os.path.join(REPO_PATH, "metrics_config.json")


jstring = '''{
  "pkt_size": 1500,
  "sim_dur": 10000000000,
  "log": {
    "out_terminal": "png",
    "out_file": "different_rtt.png",
    "cwnd": "Plot",
    "rtt": "Plot",
    "sender_losses": "Plot",
    "timeouts": "Plot",
    "link_rates": "Plot",
    "stats_intervals": [
      [
        0,
        null
      ]
    ],
    "stats_file": null,
    "link_bucket_size": 500000
  },
  "topo": {
    "topo_type": "Dumbbell",
    "link": {
      "Const": 1500000
    },
    "bufsize": "Infinite",
    "sender_groups": [
      {
        "num_senders": 1,
        "delay": 1000,
        "agg_intersend": {
          "Const": 0
        },
        "cc": "NDDProved",
        "start_time": 0,
        "tx_length": "Infinite"
      },
      {
        "num_senders": 1,
        "delay": 1000,
        "agg_intersend": {
          "Const": 0
        },
        "cc": "NDDProved",
        "start_time": 0,
        "tx_length": "Infinite"
      }
    ]
  },
  "random_seed": 0,
  "metrics_config_file": "metrics_config.json",
  "data_dir": "data"
}
'''

def run(cfg: dict):
    run_path = cfg["data_dir"]
    cfg_file = os.path.join(run_path, "config.json")
    with open(cfg_file, "w") as f:
        json.dump(cfg, f)

    start = time.time()
    subprocess.run([SIM_PATH, "file", cfg_file])
    end = time.time()
    print(f"Finished {run_path} in {end - start} seconds")


def main(args):
    # exp_path = os.path.join(OUT_PATH, "different_rtt_10ms")
    exp_path = args.output

    pool = mp.Pool(mp.cpu_count())
    # pool = None

    _cfg = json.loads(jstring)
    _cfg["metrics_config_file"] = METRICS_CONFIG_FILE
    for rttratio in [1, 2, 4, 8, 16, 32, 64, 128]:
        run_path = os.path.join(exp_path, f"rttratio_{rttratio}")
        cfg = copy.deepcopy(_cfg)
        cfg["topo"]["sender_groups"][0]["delay"] = 10000
        cfg["topo"]["sender_groups"][1]["delay"] = 10000 * rttratio
        os.makedirs(run_path, exist_ok=True)
        cfg["data_dir"] = run_path
        if pool is None:
            run(cfg)
        else:
            pool.apply_async(run, (cfg, ))

    if pool is not None:
        pool.close()
        pool.join()


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument(
        '-o', '--output', required=True,
        type=str, action='store',
        help='Output directory')
    args = parser.parse_args()
    main(args)
