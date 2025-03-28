import argparse
import ast
import multiprocessing as mp
import os
import pprint
from collections import defaultdict
from typing import Callable, List

import matplotlib.pyplot as plt
import pandas as pd

SUFFIX = "cruise.csv"


def parse_literal(element: str):
    """Converts string to literal if possible, else returns the string

    Examples
    --------
    >>> parse_literal("1.0")
    1.0
    >>> parse_literal("1")
    1
    >>> type(parse_literal("1"))
    <class 'int'>
    >>> type(parse_literal("1.0"))
    <class 'float'>
    """

    try:
        return ast.literal_eval(element)
    except ValueError:
        return element


def parse_params(s: str):
    record = {}
    param_list = s.split(':')
    for param in param_list:
        param_name, param_val = param.split('=')
        record[param_name] = parse_literal(param_val)
    return record


def plot_multi_exp(
    input_dir: str, ext: str, plot_single_exp: Callable, parallel: bool = True
):
    experiments = defaultdict(list)
    for root, _, files in os.walk(input_dir):
        for filename in files:
            # print(filename)
            # import ipdb; ipdb.set_trace()
            if (filename.endswith(ext)):
                fpath = os.path.join(root, filename)
                exp_dir = os.path.dirname(fpath)
                experiments[exp_dir].append(fpath)

    if parallel:
        pool = mp.Pool(mp.cpu_count())
        results = []
        for exp_dir, files in experiments.items():
            result = pool.apply_async(plot_single_exp, (exp_dir, files))
            results.append(result)

        pool.close()
        pool.join()
        records = [result.get() for result in results]
    else:
        records = []
        for exp_dir, files in experiments.items():
            record = plot_single_exp(exp_dir, files)
            records.append(record)

    return records


def get_steady_state_throughput(df: pd.DataFrame):
    # We look at the last 30% of the trace
    start = int(0.7 * (len(df)-1))
    end = len(df)-1
    start_rx = df.iloc[start]["end_tot_rx"]
    start_time = df.iloc[start]["end_time"]
    end_rx = df.iloc[end]["end_tot_rx"]
    end_time = df.iloc[end]["end_time"]
    return (end_rx - start_rx) / (end_time - start_time)


def plot_single_exp(input_dir: str, files: List[str]):
    cruise_dfs = {}
    for f in files:
        df = pd.read_csv(f)
        fname = os.path.basename(f)
        flow_id = fname.removesuffix(SUFFIX)
        cruise_dfs[flow_id] = df

    fig, ax = plt.subplots()
    record = {}
    for flow_id, df in cruise_dfs.items():
        ax.step(df["start_time"]/1e6, df["ack_rate"], where="post", label=flow_id)
        ss_ack_rate = get_steady_state_throughput(df)
        # mean_ack_rate = df["ack_rate"].mean()
        record[flow_id] = ss_ack_rate

    record["max_ack_rate"] = max(record.values())
    record["min_ack_rate"] = min(record.values())
    record["ratio"] = record["max_ack_rate"] / record["min_ack_rate"]
    record["input_dir"] = input_dir
    try:
        params = parse_params(os.path.basename(input_dir))
        record.update(params)
    except ValueError:
        pass
    # pprint.pprint(record)

    ax.legend()
    ax.set_xlabel("Time (s)")
    ax.set_ylabel("ACK Rate (pps)")
    ax.grid()
    fig.set_layout_engine('tight')
    fig.savefig(os.path.join(input_dir, "ack_rate.pdf"))
    plt.close(fig)

    return record


def plot_different_rtt(records, input_dir: str):
    df = pd.DataFrame(records).sort_values(["multiplier", "rttratio"])
    df["frac_short"] = df["0"]/(df["0"] + df["1"])
    print(df)

    fig, ax = plt.subplots()
    for group, gdf in df.groupby(["multiplier"]):
        ax.plot(gdf["rttratio"], gdf["frac_short"], label=group)

    rtt_ratio_list = df["rttratio"].unique()
    ax.plot(rtt_ratio_list, 1/(rtt_ratio_list + 1), label="1/(Rtprop ratio + 1)")

    ax.legend()
    ax.set_xlabel("Rtprop ratio")
    ax.set_ylabel("Fraction of link by short flow")
    ax.grid(True)
    ax.minorticks_on()
    ax.set_xscale('log', base=2)
    # ax.set_yscale('log', base=2)
    fpath = os.path.join(input_dir, "different_rtt.pdf")
    fig.savefig(fpath, bbox_inches="tight")
    plt.close(fig)


def plot_aggregate(records, agg: str, input_dir: str):
    if agg == "different_rtt":
        plot_different_rtt(records, input_dir)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "-i", "--input", required=True, type=str, action="store", help="Input directory"
    )
    parser.add_argument("-p", "--parallel", action="store_true", help="parallel")
    parser.add_argument(
        "--agg", action="store", default=None, help="Aggregate plot", type=str
    )
    args = parser.parse_args()

    records = plot_multi_exp(args.input, SUFFIX, plot_single_exp, args.parallel)
    if args.agg is not None:
        plot_aggregate(records, args.agg, args.input)


if __name__ == "__main__":
    main()
