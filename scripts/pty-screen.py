"""Runs a TUI in a pseudo-terminal, feeds keys and prints the screen (pyte).

    python3 scripts/pty-screen.py <command...> -- '["2", "wait:1", "shot:name", "\\t"]'
"""
import os, pty, select, sys, time, json
import pyte
cols, rows = 120, 40
argv = sys.argv[1:sys.argv.index("--")] if "--" in sys.argv else sys.argv[1:]
steps = json.loads(sys.argv[sys.argv.index("--")+1]) if "--" in sys.argv else []
pid, fd = pty.fork()
if pid == 0:
    os.environ["TERM"] = "xterm-256color"; os.environ["COLORTERM"] = "truecolor"
    os.execvp(argv[0], argv)
import fcntl, termios, struct
fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
screen = pyte.Screen(cols, rows); stream = pyte.ByteStream(screen)
def pump(t):
    end = time.time() + t
    while time.time() < end:
        r, _, _ = select.select([fd], [], [], 0.05)
        if r:
            try: data = os.read(fd, 65536)
            except OSError: return
            stream.feed(data)
pump(3.0)
for s in steps:
    if s.startswith("wait:"): pump(float(s[5:])); continue
    if s.startswith("shot:"):
        print("=====", s[5:]); print("\n".join(l.rstrip() for l in screen.display)); continue
    os.write(fd, s.encode().decode("unicode_escape").encode()); pump(0.8)
print("===== final"); print("\n".join(l.rstrip() for l in screen.display))
os.kill(pid, 9)
