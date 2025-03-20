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
METRICS_CONFIG_FILE = os.path.join(REPO_PATH, "metrics_config_light.json")


parking_lot_jstring = '''{
  "pkt_size": 1500,
  "sim_dur": 1000000000,
  "log": {
    "out_terminal": "png",
    "out_file": "parking_lot_ndd.png",
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
    "topo_type": "ParkingLot",
    "link": {
      "Const": 1500000
    },
    "bufsize": "Infinite",
    "sender_groups": [
      {
        "num_senders": 5,
        "delay": 10000,
        "agg_intersend": {
          "Const": 0
        },
        "cc": { "NDDProved": {} },
        "start_time": 0,
        "tx_length": "Infinite"
      }
    ]
  },
  "random_seed": 0,
}'''

different_rtt_jstring = '''{
  "pkt_size": 1500,
  "sim_dur": 1000000000,
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
      "Const": 150000000
    },
    "bufsize": "Infinite",
    "sender_groups": [
      {
        "num_senders": 1,
        "delay": 1000,
        "agg_intersend": {
          "Const": 0
        },
        "cc": { "NDDProved": {} },
        "start_time": 0,
        "tx_length": "Infinite"
      },
      {
        "num_senders": 1,
        "delay": 1000,
        "agg_intersend": {
          "Const": 0
        },
        "cc": { "NDDProved": {} },
        "start_time": 0,
        "tx_length": "Infinite"
      }
    ]
  },
  "random_seed": 0
}'''


def run(cfg: dict):
    run_path = cfg["data_dir"]
    cfg_file = os.path.join(run_path, "config.json")
    with open(cfg_file, "w") as f:
        json.dump(cfg, f)

    start = time.time()
    subprocess.run([SIM_PATH, "file", cfg_file])
    end = time.time()
    print(f"Finished {run_path} in {end - start} seconds")


def set_cc_config(cfg: dict, pdict: dict):
    cc = {
        "NDDProved": pdict
    }
    for sg in cfg["topo"]["sender_groups"]:
        sg["cc"] = cc


def different_rtt_config_list(args):
    DELAY = 1000
    exp_path = args.output
    cfg_list = []
    _cfg = json.loads(different_rtt_jstring)
    _cfg["metrics_config_file"] = METRICS_CONFIG_FILE
    # for multiplier_exp in range(-3, 10):
    for multiplier_exp in [-3]:
        multiplier = 2 ** multiplier_exp
        pdict = {
            "p_probe_multiplier": multiplier,
            "p_ub_rtterr": DELAY,
        }
        for rttratio_exp in range(1, 9):
            set_cc_config(_cfg, pdict)
            cfg = copy.deepcopy(_cfg)
            rttratio = 1 << rttratio_exp
            run_path = os.path.join(exp_path, f"rttratio={rttratio}:multiplier={multiplier}")
            cfg["topo"]["sender_groups"][0]["delay"] = DELAY
            cfg["topo"]["sender_groups"][1]["delay"] = DELAY * rttratio
            os.makedirs(run_path, exist_ok=True)
            cfg["data_dir"] = run_path
            cfg_list.append(cfg)

    return cfg_list


def parking_lot_config_list(args):
    exp_path = args.output
    cfg_list = []
    _cfg = json.loads(parking_lot_jstring)
    _cfg["metrics_config_file"] = METRICS_CONFIG_FILE
    for hops in [1, 2, 3, 4, 5, 6, 7, 8]:
        num_senders = hops + 1
        run_path = os.path.join(exp_path, f"hops_{hops}")
        cfg = copy.deepcopy(_cfg)
        cfg["topo"]["sender_groups"][0]["num_senders"] = num_senders
        os.makedirs(run_path, exist_ok=True)
        cfg["data_dir"] = run_path
        cfg_list.append(cfg)

    return cfg_list


def run_config_list(cfg_list):
    # pool = mp.Pool(mp.cpu_count())
    pool = mp.Pool(24)
    for cfg in cfg_list:
        pool.apply_async(run, (cfg, ))
    pool.close()
    pool.join()


def main(args):
    os.makedirs(args.output, exist_ok=True)
    if args.experiment_type == "parking_lot":
        parking_lot_cfg_list = parking_lot_config_list(args)
        run_config_list(parking_lot_cfg_list)
    elif args.experiment_type == "different_rtt":
        different_rtt_cfg_list = different_rtt_config_list(args)
        run_config_list(different_rtt_cfg_list)
    else:
        raise ValueError(f"Unknown experiment type: {args.experiment_type}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument(
        '-o', '--output', required=True,
        type=str, action='store',
        help='Output directory')
    parser.add_argument(
        '-e', '--experiment-type', required=True,
        type=str, action='store',
        help='Experiment type')
    args = parser.parse_args()
    main(args)
