#!/usr/bin/env python3
"""Measures `sonde tui` in a pseudo-terminal: time to the first frame, memory
(VmRSS) idle and with 100 000 rows loaded, and the --fps line while paging
through the rows. Prints Markdown rows for docs/performance.md.

    python3 scripts/perf.py target/release/sonde
"""
import os, pty, re, select, struct, sys, time, fcntl, termios
import pyte

sonde = sys.argv[1] if len(sys.argv) > 1 else "target/release/sonde"
cols, rows = 120, 40

def start(args):
    pid, fd = pty.fork()
    if pid == 0:
        os.environ.update(TERM="xterm-256color", COLORTERM="truecolor")
        os.execvp(args[0], args)
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
    screen = pyte.Screen(cols, rows)
    return pid, fd, screen, pyte.ByteStream(screen)

def pump(fd, stream, t):
    end = time.time() + t
    while time.time() < end:
        r, _, _ = select.select([fd], [], [], 0.02)
        if r:
            try: stream.feed(os.read(fd, 1 << 16))
            except OSError: return

def until(fd, stream, screen, text, timeout):
    t0 = time.time()
    while time.time() - t0 < timeout:
        pump(fd, stream, 0.02)
        if any(text in l for l in screen.display): return time.time() - t0
    raise SystemExit(f"timeout waiting for {text!r}:\n" + "\n".join(screen.display))

def rss(pid):
    for l in open(f"/proc/{pid}/status"):
        if l.startswith("VmRSS"): return int(l.split()[1]) // 1024
    return 0

def send(fd, s): os.write(fd, s.encode())

# CLI cold start
t0 = time.time()
for _ in range(10): os.system(f"{sonde} --version > /dev/null")
cli = (time.time() - t0) / 10 * 1000

samples = []
for _ in range(3):
    t0 = time.time()
    pid, fd, screen, stream = start([sonde, "tui", "--demo", "--fps"])
    first = until(fd, stream, screen, "Compatibility", 20)
    samples.append(first * 1000)
    if len(samples) < 3: os.kill(pid, 9); os.waitpid(pid, 0)
idle = rss(pid)
# Browse › demo-large, flat, load all
send(fd, "3"); until(fd, stream, screen, "Choose a bucket", 10)
for k in ["\t", "\t", "\t", "\r"]: send(fd, k); pump(fd, stream, 0.4)
send(fd, "\x1b[B"); pump(fd, stream, 0.3); send(fd, "\r")
until(fd, stream, screen, "logs/", 10)
for k in ["\t", "\t"]: send(fd, k); pump(fd, stream, 0.3)
send(fd, " ")  # the Flat switch
pump(fd, stream, 1.5)
send(fd, "\t"); pump(fd, stream, 0.3); send(fd, "\r")  # Load all
load = until(fd, stream, screen, "100,000 items", 180)
loaded = rss(pid)
# page through the table: focus it, Page Down repeatedly, read the fps line
for _ in range(5): send(fd, "\t"); pump(fd, stream, 0.2)
fps_lines = []
t0 = time.time()
pages = 0
while time.time() - t0 < 10:
    send(fd, "\x1b[6~"); pages += 1
    pump(fd, stream, 0.05)
    for line in screen.display[:3]:
        m = re.search(r"\S*fps\s.*p95.*|idle\s.*p95.*", line)
        if m: fps_lines.append(m.group(0).strip())
paged = rss(pid)
last = next((l.strip() for l in screen.display if "events-" in l), "")
os.kill(pid, 9)
print(f"| CLI start (`sonde --version`) | {cli:.1f} ms |")
print(f"| TUI start to first frame (`sonde tui --demo`), 3 runs | {', '.join(f'{s:.0f}' for s in samples)} ms |")
print(f"| Load 100 000 rows (100 ListObjectsV2 pages from the demo server) | {load:.2f} s |")
print(f"| Memory (VmRSS): idle / 100 000 rows / after paging | {idle} / {loaded} / {paged} MiB |")
print(f"| Page Down presses in 10 s | {pages} |")
print(f"| First row after paging | `{last.split()[0] if last else '?'}` |")
print("| --fps line samples | " + (" · ".join(fps_lines[-3:]) if fps_lines else "(none read)") + " |")
