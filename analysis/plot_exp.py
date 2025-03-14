import argparse
import os
from collections import defaultdict
from typing import Callable, List

import matplotlib.pyplot as plt
import pandas as pd
import multiprocessing as mp


SUFFIX = "cruise.csv"


def plot_multi_exp(input_dir: str,
                   ext: str, plot_single_exp: Callable):
    experiments = defaultdict(list)
    for root, _, files in os.walk(input_dir):
        for filename in files:
            if (filename.endswith(ext)):
                fpath = os.path.join(root, filename)
                exp_dir = os.path.dirname(fpath)
                experiments[exp_dir].append(fpath)

    pool = mp.Pool(mp.cpu_count())
    for exp_dir, files in experiments.items():
        pool.apply_async(plot_single_exp, (exp_dir, files))

    pool.close()
    pool.join()


def plot_single_exp(input_dir: str, files: List[str]):
    cruise_dfs = {}
    for f in files:
        df = pd.read_csv(f)
        fname = os.path.basename(f)
        flow_id = fname.removesuffix(SUFFIX)
        cruise_dfs[flow_id] = df

    fig, ax = plt.subplots()
    mean_ack_rates = {}
    for flow_id, df in cruise_dfs.items():
        ax.step(df["start_time"]/1e6, df["ack_rate"], where="post", label=flow_id)
        mean_ack_rate = df["ack_rate"].mean()
        mean_ack_rates[flow_id] = mean_ack_rate

    print(input_dir, mean_ack_rates)

    ax.legend()
    ax.set_xlabel("Time (s)")
    ax.set_ylabel("ACK Rate (pps)")
    ax.grid()
    fig.set_layout_engine('tight')
    fig.savefig(os.path.join(input_dir, "ack_rate.pdf"))
    plt.close(fig)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument(
        '-i', '--input', required=True,
        type=str, action='store',
        help='Input directory')
    args = parser.parse_args()

    plot_multi_exp(args.input, SUFFIX, plot_single_exp)


if __name__ == "__main__":
    main()
