import argparse
import os
from typing import Callable, List
import matplotlib.pyplot as plt
import pandas as pd


SUFFIX = "cruise.csv"


def plot_multi_exp(input_dir: str, output_dir: str,
                   ext: str, plot_single_exp: Callable):
    experiments = defaultdict(list)
    for root, _, files in os.walk(input_dir):
        for filename in files:
            if (filename.endswith(ext)):
                fpath = os.path.join(root, filename)
                exp_dir = os.path.dirname(fpath)
                experiments[exp_dir].append(fpath)

    for exp_dir, files in experiments.items():
        plot_single_exp(exp_dir, files)


def plot_single_exp(input_dir: str, files: List[str]):
    cruise_dfs = {}
    for f in files:
        df = pd.read_csv(f)
        fname = os.path.basename(f)
        flow_id = fname.removesuffix(SUFFIX)
        cruise_dfs[flow_id] = df

    fig, ax = plt.subplots()
    for flow_id, df in cruise_dfs.items():
        ax.post(df["start_time"]/1e6, df["ack_rate"], where="post", label=flow_id)

    ax.set_xlabel("Time (s)")
    ax.set_ylabel("ACK Rate (pps)")
    ax.grid()
    fig.set_tight_layout(True)
    fig.savefig(os.path.join(input_dir, "ack_rate.pdf"))
    plt.close(fig)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument(
        '-i', '--input', required=True,
        type=str, action='store',
        help='path to dmesg trace')
    args = parser.parse_args()

    plot_multi_exp(args.input, 'cruise.csv', plot_single_exp)


