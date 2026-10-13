# Wakeup latency of an ordinary task: sleep 2 ms, measure how late it wakes. Stops on SIGTERM and
# prints p50/p99/p99.9/max in microseconds as JSON.
import json, signal, sys, time
lat, run = [], True
def stop(*_):
    global run
    run = False
signal.signal(signal.SIGTERM, stop)
while run:
    t = time.perf_counter()
    time.sleep(0.002)
    lat.append((time.perf_counter() - t - 0.002) * 1e6)
lat.sort()
q = lambda p: round(lat[min(len(lat) - 1, int(len(lat) * p))])
print(json.dumps({"n": len(lat), "p50_us": q(.5), "p99_us": q(.99), "p999_us": q(.999), "max_us": round(lat[-1])}))
