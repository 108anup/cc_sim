import argparse
import os
import pandas as pd
from plotly.subplots import make_subplots
import plotly.graph_objects as go
import plotly.offline


def get_parser():
    parser = argparse.ArgumentParser(description="Plot sequence data")
    parser.add_argument("-i", "--input", type=str, help="Input file")
    parser.add_argument(
        "-a", "--around", type=int, help="plot around this time", default=None
    )
    parser.add_argument(
        "-d", "--duration", type=int, help="plot d seconds around a", default=5
    )
    parser.add_argument("-s", "--start", type=int, help="Start time", default=None)
    parser.add_argument(
        "-r", "--cwnd-update-record", type=int, help="cwnd update record", default=None
    )
    parser.add_argument("-e", "--end", type=int, help="End time", default=None)
    return parser


def infer_rtt(adf: pd.DataFrame, sdf: pd.DataFrame):
    records = []
    for idx, ack in adf.iterrows():
        match = sdf[sdf["tot_tx"] - sdf["tot_ld"] == ack["tot_rx"]]
        if len(match) == 0:
            continue
        send = match.iloc[0]
        record = {
            "send_time": send["time"],
            "ack_time": ack["time"],
            "rtt": ack["time"] - send["time"],
        }
        records.append(record)
    return pd.DataFrame(records)


def plot_timeseries(args):
    ipath = args.input
    dpath = os.path.dirname(ipath)

    df = pd.read_csv(ipath, header='infer')  # ACK df
    ack_name = os.path.basename(ipath)
    send_name = ack_name.replace('ack', 'send')
    send_path = os.path.join(dpath, send_name)
    sdf = pd.read_csv(send_path, header='infer')  # Send df

    cwnd_df = None
    if args.cwnd_update_record is not None:
        cwnd_update_name = ack_name.replace("ack", "cwnd_update")
        cwnd_update_path = os.path.join(dpath, cwnd_update_name)
        cwnd_df = pd.read_csv(cwnd_update_path, header="infer")
        update_record = cwnd_df.iloc[args.cwnd_update_record]
        args.around = update_record["now"]

    if args.around is not None:
        args.start = args.around - args.duration * 1e6
        args.end = args.around + args.duration * 1e6
    if args.start is not None:
        df = df[df["time"] >= args.start]
        sdf = sdf[sdf["time"] >= args.start]
    if args.end is not None:
        df = df[df["time"] <= args.end]
        sdf = sdf[sdf["time"] <= args.end]

    fig = make_subplots(rows=3, cols=1, shared_xaxes=True, vertical_spacing=0.02)
    start_time = min(df["time"].min(), sdf["time"].min())

    sdf["time"] = (sdf["time"] - start_time) / 1e3
    df["time"] = (df["time"] - start_time) / 1e3
    df["rtt"] = df["rtt"] / 1e3

    start_cum_seq = df["tot_rx"].min()
    df["tot_tx"] = df["tot_tx"] - start_cum_seq
    df["tot_rx"] = df["tot_rx"] - start_cum_seq
    sdf["tot_tx"] = sdf["tot_tx"] - start_cum_seq
    sdf["tot_rx"] = sdf["tot_rx"] - start_cum_seq

    fig.add_trace(
        go.Scatter(
            x=df["time"],
            y=df["tot_tx"] - df["tot_ld"],
            mode="lines",
            name="Sent",
            line={"shape": "hv"},
        ),
        row=1,
        col=1,
    )
    fig.add_trace(
        go.Scatter(
            x=df["time"],
            y=df["tot_rx"],
            mode="lines",
            name="Recvd",
            line={"shape": "hv"},
        ),
        row=1,
        col=1,
    )
    fig.add_trace(
        go.Scatter(
            x=sdf["time"],
            y=sdf["tot_tx"] - sdf["tot_ld"],
            mode="lines",
            name="Sent (on send)",
            line={"shape": "hv"},
        ),
        row=1,
        col=1,
    )
    fig.update_yaxes(title_text="Cumulative packets", row=1, col=1)

    fig.add_trace(
        go.Scatter(
            x=df["time"],
            y=df["cwnd"],
            mode="lines",
            name="cwnd",
            line={"shape": "hv"},
        ),
        row=2,
        col=1,
    )
    fig.add_trace(
        go.Scatter(
            x=df["time"],
            y=df["inflight"],
            mode="lines",
            name="inflight",
            line={"shape": "hv"},
        ),
        row=2,
        col=1,
    )
    fig.add_trace(
        go.Scatter(
            x=sdf["time"],
            y=sdf["inflight"],
            mode="lines",
            name="inflight (on send)",
            line={"shape": "hv"},
        ),
        row=2,
        col=1,
    )
    fig.update_yaxes(title_text="Packets", row=2, col=1)

    fig.add_trace(
        go.Scatter(
            x=df["time"],
            y=df["rtt"],
            mode="markers",
            name="RTT (ack)",
        ),
        row=3,
        col=1,
    )
    fig.add_trace(
        go.Scatter(
            x=df["time"]-df["rtt"],
            y=df["rtt"],
            mode="markers",
            name="RTT (sent)",
        ),
        row=3,
        col=1,
    )
    fig.update_yaxes(title_text="RTT (ms)", row=3, col=1)
    fig.update_xaxes(title_text="Time (ms)", row=3, col=1)

    # rdf = infer_rtt(df, sdf)
    # fig.add_trace(
    #     go.Scatter(
    #         x=rdf["ack_time"],
    #         y=rdf["rtt"],
    #         mode="markers",
    #         name="RTT (inferred ack)",
    #     ),
    #     row=3,
    #     col=1,
    # )
    # fig.add_trace(
    #     go.Scatter(
    #         x=rdf["send_time"],
    #         y=rdf["rtt"],
    #         mode="markers",
    #         name="RTT (inferred send)",
    #     ),
    #     row=3,
    #     col=1,
    # )

    if cwnd_df is not None:
        # Show start_seq, inflight_seq etc. vertical lines on top of RTT curve.

        # Header of cwnd update record:
        # s_probe_start_seq,s_probe_inflightmatch_seq,s_probe_first_seq,s_probe_last_seq,

        # Set index of ack df as the seqence numbert tot_rx so that we can index using it

        _sdf = sdf.set_index("tot_tx")
        _df = df.set_index("tot_rx")
        assert update_record is not None

        def get_ack_time(seq_name):
            seq = update_record[seq_name]
            return _df.loc[seq-start_cum_seq]["time"]

        def get_send_time(seq_name):
            seq = update_record[seq_name]
            return _sdf.loc[seq-start_cum_seq]["time"]

        # import ipdb; ipdb.set_trace()

        start_time = get_ack_time("s_probe_start_seq")
        inflight_match_time = get_ack_time("s_probe_inflightmatch_seq")
        first_time = get_ack_time("s_probe_first_seq")
        last_time = get_ack_time("s_probe_last_seq")

        fig.add_vline(x=start_time, row=3, col=1, name="probe start time")
        fig.add_vline(x=inflight_match_time, row=3, col=1, name="inflight match time")
        fig.add_vline(x=first_time, row=3, col=1, name="first time")
        fig.add_vline(x=last_time, row=3, col=1, name="last time")

        start_time = get_send_time("s_probe_start_seq")
        inflight_match_time = get_send_time("s_probe_inflightmatch_seq")
        first_time = get_send_time("s_probe_first_seq")
        last_time = get_send_time("s_probe_last_seq")

        fig.add_vline(x=start_time, row=3, col=1, name="probe start time (send)", line_dash="dash")
        fig.add_vline(x=inflight_match_time, row=3, col=1, name="inflight match time (send)", line_dash="dash")
        fig.add_vline(x=first_time, row=3, col=1, name="first time (send)", line_dash="dash")
        fig.add_vline(x=last_time, row=3, col=1, name="last time (send)", line_dash="dash")

    oname = os.path.basename(ipath).replace(".csv", ".html")
    opath = os.path.join(dpath, oname)
    plotly.offline.plot(fig, filename=opath)


def main():
    args = get_parser().parse_args()
    plot_timeseries(args)


if __name__ == '__main__':
    main()
