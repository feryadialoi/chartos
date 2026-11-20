"""Synthetic latency exporter for ChartOS development.

Simulates ~60 requests/s across three routes with log-normal latencies.
The median drifts on a 10-minute sine wave and a slow tail spikes every
few minutes, so histograms and heatmaps show shape and movement.
Standard library only.
"""
import math
import random
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

BUCKETS = [0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]
ROUTES = {  # route: (base median seconds, sigma, share of traffic)
    "/api/orders": (0.08, 0.6, 0.5),
    "/api/search": (0.25, 0.8, 0.3),
    "/api/report": (1.2, 0.5, 0.2),
}
RATE = 60.0

lock = threading.Lock()
counts = {r: [0] * len(BUCKETS) for r in ROUTES}
totals = {r: 0 for r in ROUTES}
sums = {r: 0.0 for r in ROUTES}
errors = {r: 0 for r in ROUTES}


def observe(route, value):
    with lock:
        for i, b in enumerate(BUCKETS):
            if value <= b:
                counts[route][i] += 1
        totals[route] += 1
        sums[route] += value
        if value > 2.0 and random.random() < 0.3:
            errors[route] += 1


def simulate():
    names = list(ROUTES)
    weights = [ROUTES[r][2] for r in names]
    while True:
        t = time.time()
        drift = 1 + 0.6 * math.sin(2 * math.pi * t / 600)
        spike = (t % 240) < 30  # 30s of slow tail every 4 minutes
        for _ in range(int(RATE / 10)):
            route = random.choices(names, weights)[0]
            median, sigma, _ = ROUTES[route]
            value = random.lognormvariate(math.log(median * drift), sigma)
            if spike and random.random() < 0.2:
                value *= 6
            observe(route, value)
        time.sleep(0.1)


def render():
    out = [
        "# HELP demo_request_duration_seconds Simulated request latency.",
        "# TYPE demo_request_duration_seconds histogram",
    ]
    with lock:
        for r in ROUTES:
            for b, c in zip(BUCKETS, counts[r]):
                out.append(f'demo_request_duration_seconds_bucket{{route="{r}",le="{b}"}} {c}')
            out.append(f'demo_request_duration_seconds_bucket{{route="{r}",le="+Inf"}} {totals[r]}')
            out.append(f'demo_request_duration_seconds_sum{{route="{r}"}} {sums[r]}')
            out.append(f'demo_request_duration_seconds_count{{route="{r}"}} {totals[r]}')
        out += ["# HELP demo_errors_total Simulated failed requests.", "# TYPE demo_errors_total counter"]
        for r in ROUTES:
            out.append(f'demo_errors_total{{route="{r}"}} {errors[r]}')
    return ("\n".join(out) + "\n").encode()


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path != "/metrics":
            self.send_error(404)
            return
        body = render()
        self.send_response(200)
        self.send_header("Content-Type", "text/plain; version=0.0.4")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        pass


if __name__ == "__main__":
    threading.Thread(target=simulate, daemon=True).start()
    ThreadingHTTPServer(("0.0.0.0", 8000), Handler).serve_forever()
